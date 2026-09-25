//! Raw evdev for the panel frontend (specs/recovery-menu.md §4): every `/dev/input/event*`,
//! `poll(2)`, `input_event` records. Android recovery's semantics (recovery_ui/ui.cpp
//! ProcessKey): a key REGISTERS ON RELEASE, auto-repeat is ignored, a press held ≥ long_press_ms
//! registers as `Long` on release. So a held key is one input, whatever the kernel's repeat rate.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const EV_KEY: u16 = 1;
const INPUT_DIR: &str = "/dev/input";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Short,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: u16,
    pub press: Press,
}

pub struct Devices {
    files: Vec<(PathBuf, File)>,
    /// inotify on /dev/input: button drivers probe whenever udev's coldplug reaches them, which
    /// on the VM (i8042/atkbd) and on gpio-keys targets alike is after this program has started.
    watch: Option<File>,
    down: Vec<(u16, Instant)>,
    long_press: Duration,
}

impl Devices {
    /// Open every event device present now and watch /dev/input for the ones that appear later.
    pub fn open_all(long_press_ms: u64) -> Devices {
        let watch = unsafe {
            let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
            if fd < 0 {
                None
            } else {
                let dir = std::ffi::CString::new(INPUT_DIR).unwrap();
                if libc::inotify_add_watch(fd, dir.as_ptr(), libc::IN_CREATE | libc::IN_ATTRIB | libc::IN_DELETE) < 0 {
                    libc::close(fd);
                    None
                } else {
                    Some(File::from_raw_fd(fd))
                }
            }
        };
        let mut d = Devices { files: Vec::new(), watch, down: Vec::new(), long_press: Duration::from_millis(long_press_ms) };
        d.rescan();
        d
    }

    /// Open the event devices not yet open (in name order); forget the ones that vanished.
    fn rescan(&mut self) {
        self.files.retain(|(p, _)| p.exists());
        if let Ok(rd) = fs::read_dir(INPUT_DIR) {
            let mut names: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("event")).unwrap_or(false)).collect();
            names.sort();
            for p in names {
                if self.files.iter().any(|(q, _)| *q == p) {
                    continue;
                }
                if let Ok(f) = File::open(&p) {
                    eprintln!("mura-recovery panel: reading {}", p.display());
                    self.files.push((p, f));
                }
            }
        }
    }

    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// Block until a key REGISTERS (release), and return it. `None` only if no device is open
    /// and none can be waited for.
    pub fn next_key(&mut self) -> Option<Key> {
        let ev_size = std::mem::size_of::<libc::input_event>();
        let mut buf = vec![0u8; ev_size * 64];
        loop {
            if self.files.is_empty() && self.watch.is_none() {
                return None;
            }
            let mut fds: Vec<libc::pollfd> = self.files.iter().map(|(_, f)| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
            if let Some(w) = &self.watch {
                fds.push(libc::pollfd { fd: w.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            }
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
            if n < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return None;
            }
            // /dev/input changed: drain the notification and re-scan
            if let Some(w) = &mut self.watch {
                if fds.last().map(|p| p.revents & libc::POLLIN != 0).unwrap_or(false) {
                    let mut scratch = [0u8; 4096];
                    while let Ok(k) = w.read(&mut scratch) {
                        if k == 0 {
                            break;
                        }
                    }
                    self.rescan();
                    continue;
                }
            }
            let mut gone = Vec::new();
            for i in 0..self.files.len() {
                let pfd = fds[i];
                if pfd.revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP) == 0 {
                    continue;
                }
                let got = match self.files[i].1.read(&mut buf) {
                    Ok(g) => g,
                    Err(_) => {
                        gone.push(i); // unplugged: ENODEV
                        continue;
                    }
                };
                for chunk in buf[..got].chunks_exact(ev_size) {
                    // SAFETY: the kernel writes whole input_event structs; the chunk is one of them.
                    let ev: libc::input_event = unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const libc::input_event) };
                    if ev.type_ != EV_KEY {
                        continue;
                    }
                    match ev.value {
                        1 => {
                            // press: remember when; nothing registers yet
                            self.down.retain(|(c, _)| *c != ev.code);
                            self.down.push((ev.code, Instant::now()));
                        }
                        0 => {
                            // release: register, short or long
                            if let Some(pos) = self.down.iter().position(|(c, _)| *c == ev.code) {
                                let (_, since) = self.down.remove(pos);
                                let press = if since.elapsed() >= self.long_press { Press::Long } else { Press::Short };
                                return Some(Key { code: ev.code, press });
                            }
                            // a release without a seen press (held across our start): ignore
                        }
                        _ => {} // 2 = auto-repeat: ignored
                    }
                }
            }
            for i in gone.into_iter().rev() {
                self.files.remove(i);
            }
        }
    }
}
