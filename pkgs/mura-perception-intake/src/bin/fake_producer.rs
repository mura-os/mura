//! intake-fake-producer — the §8 harness's producer: a perception service with no camera.
//!
//! Allocates a fixed image table (udmabuf), registers it once over the control socket (REGISTER
//! + REGISTER_MORE, ≤16 fds a datagram), then publishes stamped generations at a rate into the
//! memfd register, dropping with OVERRUN when no slot is reclaimable (§4). Never blocks on the
//! consumer: the ack is read asynchronously, notifications are sent non-blocking.
//!
//!   --socket PATH            the consumer's SEQPACKET socket (required)
//!   --layer environment|hand_top   default environment
//!   --max-in-flight N        pool = 2 + N slots (default 2)
//!   --rate-hz HZ             publication rate (default 60)
//!   --generations N          publish N then GOODBYE and exit (default: until SIGTERM)
//!   --hold-acquire-from G    from generation G on, never signal acquire points (§8 item 4)
//!   --recalibrate-at G       bump calibration_ver at generation G (§8 item 3)
//!   --restart-epoch-after G  after G generations tear the session down without GOODBYE and
//!                            register again with producer_epoch + 1 (§6)
//!   --send-unknown-type      after registering, send a record of type 99 (must be ignored)
//!   --version-bump           register with version+1 (must be refused; exit 3)
//!   --width W --height H     image size (default 64×64, 4 bytes/px)
//!   --report PATH            key=value counters on exit

#[path = "../harness.rs"]
mod harness;

use harness::Report;
use perception_intake::kernel::{self, Drm, Mapping, Timeline, Udmabuf};
use perception_intake::record::{ColourGroup, GeometryGroup, ImageRef, Record, Stamp, FLAG_COMPLETE, MAX_IMAGES};
use perception_intake::register::{Declared, Pool, ProducerRegister};
use perception_intake::usepage::ProducerUsePage;
use perception_intake::seqpacket::{Socket, MAX_DATAGRAM};
use perception_intake::wire::{self, ImageEntry, Message, ENTRIES_PER_MORE, ENTRIES_PER_REGISTER};
use perception_intake::{layer_name, parse_layer, LAYER_ENVIRONMENT, LAYER_HAND_TOP};
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_term(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

const FOURCC_ABGR8888: u32 = 0x3441_4241; // DRM_FORMAT_ABGR8888 'AB24'
const FOURCC_R8: u32 = 0x2020_3852; // DRM_FORMAT_R8 'R8  '
const FOURCC_R16: u32 = 0x2036_3152; // DRM_FORMAT_R16
const USAGE_COLOUR: u32 = 1;
const USAGE_DEPTH: u32 = 2;
const USAGE_CONFIDENCE: u32 = 3;
const USAGE_GUIDE: u32 = 4;
const USAGE_ALPHA_F: u32 = 5;
const USAGE_ALPHA: u32 = 6;

struct Args {
    socket: String,
    layer: u32,
    max_in_flight: u32,
    rate_hz: f64,
    generations: Option<u64>,
    hold_acquire_from: Option<u64>,
    recalibrate_at: Option<u64>,
    restart_epoch_after: Option<u64>,
    send_unknown: bool,
    version_bump: bool,
    width: u32,
    height: u32,
    report: Option<String>,
}

fn args() -> Args {
    let mut a = Args {
        socket: String::new(),
        layer: LAYER_ENVIRONMENT,
        max_in_flight: 2,
        rate_hz: 60.0,
        generations: None,
        hold_acquire_from: None,
        recalibrate_at: None,
        restart_epoch_after: None,
        send_unknown: false,
        version_bump: false,
        width: 64,
        height: 64,
        report: None,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        *i += 1;
        v.get(*i).cloned().unwrap_or_else(|| usage())
    };
    while i < v.len() {
        match v[i].as_str() {
            "--socket" => a.socket = next(&mut i),
            "--layer" => a.layer = parse_layer(&next(&mut i)).unwrap_or_else(|| usage()),
            "--max-in-flight" => a.max_in_flight = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--rate-hz" => a.rate_hz = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--generations" => a.generations = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--hold-acquire-from" => a.hold_acquire_from = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--recalibrate-at" => a.recalibrate_at = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--restart-epoch-after" => a.restart_epoch_after = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--send-unknown-type" => a.send_unknown = true,
            "--version-bump" => a.version_bump = true,
            "--width" => a.width = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--height" => a.height = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--report" => a.report = Some(next(&mut i)),
            _ => usage(),
        }
        i += 1;
    }
    if a.socket.is_empty() {
        usage();
    }
    a
}

fn usage() -> ! {
    eprintln!("usage: intake-fake-producer --socket PATH [--layer environment|hand_top] [--max-in-flight N] [--rate-hz HZ] [--generations N] [--hold-acquire-from G] [--recalibrate-at G] [--restart-epoch-after G] [--send-unknown-type] [--version-bump] [--width W --height H] [--report PATH]");
    std::process::exit(2)
}

/// One registered image: its memfd (our write mapping), the dmabuf the consumer gets, and its
/// two timelines.
struct Image<'d> {
    entry: ImageEntry,
    _mem: OwnedFd,
    map: Mapping,
    dmabuf: OwnedFd,
    acquire: Timeline<'d>,
    release: Timeline<'d>,
    acquire_fd: OwnedFd,
    release_fd: OwnedFd,
}

/// Everything one producer_epoch owns (§6: a restart is a new session, a higher epoch).
struct Session<'d> {
    epoch: u64,
    register: ProducerRegister,
    pool: Pool,
    images: Vec<Image<'d>>,
    per_set: usize,
    slot_count: u32,
    /// The consumer's declarations, from the ack; until then nobody uses anything.
    use_page: Option<ProducerUsePage>,
}

fn image_kinds(layer: u32) -> Vec<(u32, u32, u32)> {
    // (usage, fourcc, bytes per pixel) per view; the table is kinds × 2 views per set
    match layer {
        LAYER_HAND_TOP => vec![(USAGE_ALPHA_F, FOURCC_ABGR8888, 4), (USAGE_ALPHA, FOURCC_R8, 1), (USAGE_DEPTH, FOURCC_R16, 2)],
        _ => vec![(USAGE_COLOUR, FOURCC_ABGR8888, 4), (USAGE_DEPTH, FOURCC_R16, 2), (USAGE_CONFIDENCE, FOURCC_R8, 1), (USAGE_GUIDE, FOURCC_R8, 1)],
    }
}

impl<'d> Session<'d> {
    fn new(drm: &'d Drm, udma: &Udmabuf, a: &Args, epoch: u64) -> std::io::Result<Session<'d>> {
        let slot_count = 2 + a.max_in_flight;
        let register = ProducerRegister::create(a.layer, epoch, slot_count)?;
        let kinds = image_kinds(a.layer);
        let per_set = kinds.len() * 2;
        assert!(per_set <= MAX_IMAGES * 2);
        let mut images = Vec::new();
        for _set in 0..slot_count {
            for _view in 0..2u32 {
                for (usage, fourcc, bpp) in &kinds {
                    let stride = a.width * bpp;
                    let size = (stride * a.height) as usize;
                    let mem = kernel::memfd("mura-intake-image", size)?;
                    let map = Mapping::map(mem.as_raw_fd(), kernel::page_round(size), true)?;
                    let dmabuf = udma.create(&mem, size)?;
                    let acquire = Timeline::create(drm)?;
                    let release = Timeline::create(drm)?;
                    let acquire_fd = acquire.export()?;
                    let release_fd = release.export()?;
                    let slot_index = images.len() as u32;
                    images.push(Image {
                        entry: ImageEntry { slot_index, fourcc: *fourcc, modifier: 0, width: a.width, height: a.height, stride, offset: 0, size: kernel::page_round(size) as u32, usage: *usage },
                        _mem: mem,
                        map,
                        dmabuf,
                        acquire,
                        release,
                        acquire_fd,
                        release_fd,
                    });
                }
            }
        }
        Ok(Session { epoch, register, pool: Pool::new(slot_count), images, per_set, slot_count, use_page: None })
    }

    /// REGISTER + REGISTER_MORE: the memfd, then every image's dmabuf/acquire/release fds.
    fn register(&self, sock: &Socket, a: &Args, version: u32) -> std::io::Result<()> {
        let entries: Vec<ImageEntry> = self.images.iter().map(|i| i.entry).collect();
        let first = &entries[..entries.len().min(ENTRIES_PER_REGISTER)];
        let mut fds: Vec<RawFd> = vec![self.register.fd()];
        for i in &self.images[..first.len()] {
            fds.extend([i.dmabuf.as_raw_fd(), i.acquire_fd.as_raw_fd(), i.release_fd.as_raw_fd()]);
        }
        let m = Message::Register {
            producer_epoch: self.epoch,
            layer_kind: a.layer,
            max_in_flight: a.max_in_flight,
            image_count: entries.len() as u32,
            register_size: self.register.size() as u32,
            name: "intake-fake-producer".into(),
            entries: first.to_vec(),
        };
        sock.send(&m.encode_with_version(version), &fds)?;
        let mut at = first.len();
        while at < entries.len() {
            let chunk = &entries[at..(at + ENTRIES_PER_MORE).min(entries.len())];
            let mut fds: Vec<RawFd> = Vec::new();
            for i in &self.images[at..at + chunk.len()] {
                fds.extend([i.dmabuf.as_raw_fd(), i.acquire_fd.as_raw_fd(), i.release_fd.as_raw_fd()]);
            }
            sock.send(&Message::RegisterMore { producer_epoch: self.epoch, fd_base: at as u32, entries: chunk.to_vec() }.encode(), &fds)?;
            at += chunk.len();
        }
        Ok(())
    }

    /// Last signalled release point of every image, one ioctl.
    fn release_points(&self, drm: &Drm) -> Vec<u64> {
        let handles: Vec<u32> = self.images.iter().map(|i| i.release.handle).collect();
        let mut out = vec![0u64; handles.len()];
        if let Err(e) = kernel::query_many(drm, &handles, &mut out) {
            eprintln!("producer: syncobj query failed: {e}");
        }
        out
    }

    /// Publish generation `gen` into image set `slot`: stamps, record, latest index, acquires.
    fn publish(&mut self, slot: u32, gen: u64, calibration_ver: u32, layer: u32, hold_acquire: bool) -> Record {
        let base = slot as usize * self.per_set;
        let stamp = Stamp { producer_epoch: self.epoch, generation: gen, calibration_ver, slot_index: 0 };
        for i in base..base + self.per_set {
            let img = &mut self.images[i];
            Stamp { slot_index: img.entry.slot_index, ..stamp }.write(img.map.bytes_mut());
        }
        let now = kernel::now_ns();
        let mut rec = Record { layer_kind: layer, flags: FLAG_COMPLETE, generation: gen, producer_epoch: self.epoch, ..Default::default() };
        let refs: Vec<ImageRef> = (base..base + self.per_set).map(|i| ImageRef { slot_index: i as u32, acquire_point: gen, release_point: gen }).collect();
        // colour group = the colour-class images of both views; geometry = the rest
        let kinds = image_kinds(layer);
        let mut c = ColourGroup { t_colour_ns: now, pose_colour: [0.0, 1.6, 0.0, 0.0, 0.0, 0.0, 1.0], calibration_ver, colour_space: 1, exposure_us: 8000, gain: 1.0, distortion_ref: 0, ..Default::default() };
        let mut g = GeometryGroup {
            t_geometry_ns: now.saturating_sub(4_000_000), // dual rate: geometry lags 4 ms
            pose_geometry: [0.0, 1.6, 0.0, 0.0, 0.0, 0.0, 1.0],
            calibration_ver,
            t_geom_to_colour: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            depth_fourcc: FOURCC_R16,
            depth_params: 16,
            near_m: 0.1,
            far_m: 20.0,
            min_stored: 0.0,
            max_stored: 65535.0,
            reversed: 0,
            intrinsics: [400.0, 400.0, 32.0, 32.0],
            baseline_m: 0.064,
            ..Default::default()
        };
        for (k, r) in refs.iter().enumerate() {
            let usage = kinds[k % kinds.len()].0;
            let colour_class = usage == USAGE_COLOUR || usage == USAGE_ALPHA_F || usage == USAGE_ALPHA;
            let (grp_count, grp_images) = if colour_class { (&mut c.image_count, &mut c.images) } else { (&mut g.image_count, &mut g.images) };
            if (*grp_count as usize) < MAX_IMAGES {
                grp_images[*grp_count as usize] = *r;
                *grp_count += 1;
            }
        }
        rec.colour = c;
        rec.geometry = g;
        self.register.publish(slot, &rec);
        self.pool.set(slot, rec);
        if !hold_acquire {
            for i in base..base + self.per_set {
                if let Err(e) = self.images[i].acquire.signal(gen) {
                    eprintln!("producer: acquire signal failed: {e}");
                }
            }
        }
        rec
    }
}

fn vm_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok())))
        .unwrap_or(0)
}

fn main() {
    let a = args();
    // SAFETY: installing a trivial async-signal-safe handler.
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
    let drm = Drm::open_any().unwrap_or_else(|e| {
        eprintln!("producer: {e}");
        std::process::exit(1)
    });
    let udma = Udmabuf::open().unwrap_or_else(|e| {
        eprintln!("producer: /dev/udmabuf: {e}");
        std::process::exit(1)
    });
    let sock = Socket::connect(&a.socket).unwrap_or_else(|e| {
        eprintln!("producer: connect {}: {e}", a.socket);
        std::process::exit(1)
    });

    let mut report = Report::default();
    let mut epoch: u64 = 1;
    let mut session = Session::new(&drm, &udma, &a, epoch).unwrap_or_else(|e| {
        eprintln!("producer: session: {e}");
        std::process::exit(1)
    });
    let version = if a.version_bump { wire::VERSION + 1 } else { wire::VERSION };
    session.register(&sock, &a, version).unwrap_or_else(|e| {
        eprintln!("producer: register: {e}");
        std::process::exit(1)
    });
    eprintln!("producer: epoch {epoch} registered {} images ({} slots) for layer {}", session.images.len(), session.slot_count, layer_name(a.layer));
    if a.send_unknown {
        let _ = sock.send(&Message::Unknown { ty: 99 }.encode(), &[]);
    }

    let period_ns = (1e9 / a.rate_hz) as u64;
    let mut next_at = kernel::now_ns();
    let mut gen: u64 = 0;
    let mut published: u64 = 0;
    let mut dropped: u64 = 0;
    let mut overrun_reports: u64 = 0;
    let mut epochs: u64 = 1;
    let mut acked = false;
    let mut calibration_ver: u32 = 1;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let mut exit_code = 0;

    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        // the ack (asynchronous, §5 rule 2) and anything else the consumer says
        loop {
            match sock.recv(&mut buf, true) {
                Ok(Some(d)) => match Message::decode(&buf[..d.len]) {
                    Ok(Message::RegisterAck { producer_epoch, status, max_in_flight }) => {
                        // epoch 0 = the consumer could not read ours (a future-version REGISTER)
                        if producer_epoch == session.epoch || producer_epoch == 0 {
                            if status == wire::ACK_OK && producer_epoch != 0 {
                                acked = true;
                                match d.fds.first().map(|f| ProducerUsePage::open(f.as_raw_fd())) {
                                    Some(Ok(p)) => session.use_page = Some(p),
                                    Some(Err(e)) => eprintln!("producer: consumer's use page unreadable: {e}"),
                                    None => eprintln!("producer: ack without a use page"),
                                }
                                eprintln!("producer: acked (consumer max_in_flight {max_in_flight})");
                            } else {
                                eprintln!("producer: registration refused by the consumer");
                                exit_code = 3;
                                STOP.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("producer: bad datagram from consumer: {e:?}"),
                },
                Ok(None) => {
                    eprintln!("producer: consumer gone");
                    STOP.store(true, Ordering::Relaxed);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("producer: recv: {e}");
                    STOP.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if let Some(n) = a.generations {
            if gen >= n {
                let _ = sock.send(&Message::Goodbye { producer_epoch: session.epoch }.encode(), &[]);
                eprintln!("producer: {n} generations published, goodbye");
                break;
            }
        }
        if let Some(r) = a.restart_epoch_after {
            if gen == r && epochs == 1 {
                // device loss: everything of the old epoch goes away without a word (§6)
                eprintln!("producer: simulating device loss after generation {gen}; re-registering as epoch {}", epoch + 1);
                epoch += 1;
                epochs += 1;
                session = Session::new(&drm, &udma, &a, epoch).unwrap_or_else(|e| {
                    eprintln!("producer: session: {e}");
                    std::process::exit(1)
                });
                session.register(&sock, &a, wire::VERSION).unwrap_or_else(|e| {
                    eprintln!("producer: register: {e}");
                    std::process::exit(1)
                });
                acked = false;
                gen = 0;
            }
        }
        // publish one generation (the consumer's ack is not waited for: §5 rule 2; generations
        // before the ack are simply not selected by a conforming consumer)
        let _ = acked;
        gen += 1;
        if a.recalibrate_at == Some(gen) {
            calibration_ver += 1;
            eprintln!("producer: calibration_ver -> {calibration_ver} at generation {gen}");
        }
        let points = session.release_points(&drm);
        let slot = {
            let Session { pool, register, use_page, .. } = &mut session;
            // the two-flag exchange: announce, then read the consumer's declaration
            pool.reclaim(
                |img| points[img as usize],
                |g| {
                    register.set_reclaiming(g);
                    match use_page {
                        None => Declared::Free,
                        Some(p) if p.intent() == g => Declared::Intent,
                        Some(p) if p.pending_contains(g) => Declared::Pending,
                        Some(_) => Declared::Free,
                    }
                },
            )
        };
        match slot {
            Some(slot) => {
                let hold = a.hold_acquire_from.map(|g| gen >= g).unwrap_or(false);
                let rec = session.publish(slot, gen, calibration_ver, a.layer, hold);
                session.register.clear_reclaiming(); // after the write and the new latest
                published += 1;
                let _ = sock.send(&Message::Generation(rec).encode(), &[]); // a notification; EAGAIN is fine
            }
            None => {
                session.register.clear_reclaiming();
                dropped += 1;
                let _ = sock.send(&Message::Overrun { producer_epoch: session.epoch, dropped_total: dropped }.encode(), &[]);
                overrun_reports += 1;
            }
        }
        // pace (the producer may sleep: it never waits on the consumer, only on its clock)
        next_at += period_ns;
        let now = kernel::now_ns();
        if next_at > now {
            let d = next_at - now;
            let ts = libc::timespec { tv_sec: (d / 1_000_000_000) as _, tv_nsec: (d % 1_000_000_000) as _ };
            // SAFETY: plain nanosleep.
            unsafe { libc::nanosleep(&ts, std::ptr::null_mut()) };
        } else if now - next_at > period_ns * 10 {
            next_at = now; // fell far behind (a stopped VM): do not burst
        }
    }

    report.set("layer", layer_name(a.layer));
    report.set("epochs", epochs);
    report.set("published", published);
    report.set("dropped", dropped);
    report.set("overrun_reports", overrun_reports);
    report.set("reclaimed", session.pool.reclaimed);
    report.set("slots", session.slot_count);
    report.set("images", session.images.len());
    report.set("last_generation", gen);
    report.set("vm_rss_kb", vm_rss_kb());
    report.set("exit_code", exit_code);
    if let Some(p) = &a.report {
        report.write(p);
    }
    eprint!("{}", report.render());
    std::process::exit(exit_code);
}
