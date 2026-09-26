//! intake-test-consumer — the §8 harness's consumer: zxr's intake with no compositor.
//!
//! Listens on a SEQPACKET socket, registers whatever producer connects (REGISTER/REGISTER_MORE
//! → REGISTER_ACK), maps its register read-only, and runs composition *passes*: one
//! acquire-ordered read of the latest index, one syncobj query for the generation's acquire
//! points, select if all signalled else keep the current (§4/§5), verify the image stamps
//! against the record, "submit GPU work" that signals the release points later (§4 reclamation;
//! per image, optionally out of order — §8 item 5), retire a dead or superseded epoch only after
//! every use completed (§6). Every pass is a fixed, bounded set of non-blocking syscalls:
//! recvmsg(MSG_DONTWAIT) drain, one SYNCOBJ_QUERY, SYNCOBJ_TIMELINE_SIGNAL per completed image.
//! `PASSES_BEGIN`/`PASSES_END` on stderr delimit the window a tracer may judge (§8 item 4).
//!
//!   --socket PATH            listen here (required unless --probe)
//!   --passes N               run N passes then exit (default: until the producer is gone and
//!                            retired, --until-producer-gone)
//!   --pace-hz HZ             passes per second (default 90); --spin paces without sleeping
//!   --max-in-flight N        what we declare in the ack (default 2)
//!   --gpu-ms MS              a fake GPU use completes MS after submission (default 8)
//!   --release-order normal|swap   swap: every other use releases its second image 3× later
//!   --hold-release-ms MS     after producer death, keep uses pending this long before completing
//!   --exceed-in-flight N     MISBEHAVE: hold up to N uses although we declared max_in_flight —
//!                            the only way a conforming producer ever drops (§8 item 2)
//!   --report PATH            key=value counters on exit
//!   --probe                  print the DRM node/caps and udmabuf presence; exit 0 if usable

#[path = "../harness.rs"]
mod harness;

use harness::{open_fd_count, Report};
use perception_intake::kernel::{self, Drm, Mapping, Timeline, NONZERO_TIMEOUT_WAITS};
use perception_intake::record::{Record, Stamp, STAMP_SIZE};
use perception_intake::register::ConsumerRegister;
use perception_intake::seqpacket::{Socket, MAX_DATAGRAM};
use perception_intake::usepage::ConsumerUsePage;
use perception_intake::wire::{self, DecodeError, ImageEntry, Message, FDS_PER_ENTRY};
use perception_intake::layer_name;
use std::io::Write;
use std::os::unix::io::{AsRawFd, OwnedFd};
use std::sync::atomic::Ordering;

struct Args {
    socket: String,
    passes: Option<u64>,
    pace_hz: f64,
    spin: bool,
    max_in_flight: u32,
    gpu_ms: u64,
    swap: bool,
    hold_release_ms: u64,
    exceed_in_flight: Option<usize>,
    report: Option<String>,
    probe: bool,
}

fn usage() -> ! {
    eprintln!("usage: intake-test-consumer --socket PATH [--passes N] [--pace-hz HZ] [--spin] [--max-in-flight N] [--gpu-ms MS] [--release-order normal|swap] [--hold-release-ms MS] [--report PATH] | --probe");
    std::process::exit(2)
}

fn args() -> Args {
    let mut a = Args { socket: String::new(), passes: None, pace_hz: 90.0, spin: false, max_in_flight: 2, gpu_ms: 8, swap: false, hold_release_ms: 0, exceed_in_flight: None, report: None, probe: false };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        *i += 1;
        v.get(*i).cloned().unwrap_or_else(|| usage())
    };
    while i < v.len() {
        match v[i].as_str() {
            "--socket" => a.socket = next(&mut i),
            "--passes" => a.passes = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--until-producer-gone" => a.passes = None,
            "--pace-hz" => a.pace_hz = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--spin" => a.spin = true,
            "--max-in-flight" => a.max_in_flight = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--gpu-ms" => a.gpu_ms = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--release-order" => a.swap = next(&mut i) == "swap",
            "--hold-release-ms" => a.hold_release_ms = next(&mut i).parse().unwrap_or_else(|_| usage()),
            "--exceed-in-flight" => a.exceed_in_flight = Some(next(&mut i).parse().unwrap_or_else(|_| usage())),
            "--report" => a.report = Some(next(&mut i)),
            "--probe" => a.probe = true,
            _ => usage(),
        }
        i += 1;
    }
    if a.socket.is_empty() && !a.probe {
        usage();
    }
    a
}

fn probe() -> ! {
    let mut ok = true;
    match Drm::open_any() {
        Ok(d) => println!("drm={} syncobj={} syncobj_timeline={}", d.path, d.cap(kernel::DRM_CAP_SYNCOBJ).unwrap_or(0), d.cap(kernel::DRM_CAP_SYNCOBJ_TIMELINE).unwrap_or(0)),
        Err(e) => {
            println!("drm=none error={e}");
            ok = false;
        }
    }
    match kernel::Udmabuf::open() {
        Ok(_) => println!("udmabuf=yes"),
        Err(e) => {
            println!("udmabuf=no error={e}");
            ok = false;
        }
    }
    std::process::exit(if ok { 0 } else { 1 })
}

/// A registered image on our side: the dmabuf (mapped read-only for the stamp checks) and the
/// imported timelines.
struct Image<'d> {
    _entry: ImageEntry,
    _dmabuf: OwnedFd,
    map: Mapping,
    acquire: Timeline<'d>,
    release: Timeline<'d>,
}

/// A submitted "GPU use" of one generation: per image, when its release point gets signalled.
struct Use {
    generation: u64,
    images: Vec<(u32, u64, u64, bool)>, // (image slot, release_point, complete_at_ns, done)
    submitted_at: u64,
}

/// One producer_epoch as we see it (§6): registering, active, or dying.
struct Epoch<'d> {
    epoch: u64,
    layer_kind: u32,
    image_count: u32,
    register: Option<ConsumerRegister>,
    images: Vec<Option<Image<'d>>>,
    received: u32,
    acked: bool,
    current: Option<(u32, Record)>,
    uses: Vec<Use>,
    dying_since: Option<u64>,
    last_calibration: Option<u32>,
    /// Our declarations to the producer (usepage): intent + pending uses.
    use_page: Option<ConsumerUsePage>,
    /// Submission time of the newest use that has completed an image: a completion of an
    /// older use after it is an out-of-order release (§8 item 5).
    newest_completed_submit: u64,
}

impl<'d> Epoch<'d> {
    fn complete(&self) -> bool {
        self.received == self.image_count && self.register.is_some()
    }
    fn add_entries(&mut self, drm: &'d Drm, base: usize, entries: &[ImageEntry], fds: Vec<OwnedFd>) -> Result<(), String> {
        if fds.len() != entries.len() * FDS_PER_ENTRY {
            return Err(format!("{} entries but {} fds", entries.len(), fds.len()));
        }
        let mut fds = fds.into_iter();
        for (k, e) in entries.iter().enumerate() {
            let idx = base + k;
            if idx != e.slot_index as usize || idx >= self.images.len() {
                return Err(format!("entry {idx} declares slot {}", e.slot_index));
            }
            let dmabuf = fds.next().unwrap();
            let acq = fds.next().unwrap();
            let rel = fds.next().unwrap();
            let map = Mapping::map(dmabuf.as_raw_fd(), e.size as usize, false).map_err(|e| format!("map dmabuf: {e}"))?;
            let acquire = Timeline::import(drm, acq.as_raw_fd()).map_err(|e| format!("import acquire: {e}"))?;
            let release = Timeline::import(drm, rel.as_raw_fd()).map_err(|e| format!("import release: {e}"))?;
            if self.images[idx].is_none() {
                self.received += 1;
            }
            self.images[idx] = Some(Image { _entry: *e, _dmabuf: dmabuf, map, acquire, release });
        }
        Ok(())
    }
    fn pending_uses(&self) -> usize {
        self.uses.iter().filter(|u| u.images.iter().any(|i| !i.3)).count()
    }
}

#[derive(Default)]
struct Counters {
    passes: u64,
    selections: u64,
    fallback_reused_current: u64,
    layer_absent: u64,
    generations_skipped: u64,
    stamp_mismatch: u64,
    calibration_mismatch: u64,
    calibration_changes_seen: u64,
    overwritten_while_in_use: u64,
    selection_backed_off: u64,
    uses_submitted: u64,
    uses_completed: u64,
    out_of_order_releases: u64,
    max_pending_uses: u64,
    fallback_in_flight_full: u64,
    pending_table_full_refused: u64,
    overrun_reports: u64,
    overrun_dropped_total: u64,
    generation_notifications: u64,
    unknown_ignored: u64,
    registration_failures: u64,
    epochs_seen: u64,
    epochs_retired: u64,
    goodbyes: u64,
    producer_gone: u64,
    last_generation: u64,
    fds_baseline: usize,
    fds_registered: usize,
    fds_at_death: usize,
    fds_after_retire: usize,
    retire_delay_ms: u64,
}

fn stamp_of(img: &Image) -> Stamp {
    Stamp::read(&img.map.bytes()[..STAMP_SIZE])
}

fn main() {
    let a = args();
    if a.probe {
        probe();
    }
    let drm = Drm::open_any().unwrap_or_else(|e| {
        eprintln!("consumer: {e}");
        std::process::exit(1)
    });
    let listener = Socket::listen(&a.socket).unwrap_or_else(|e| {
        eprintln!("consumer: listen {}: {e}", a.socket);
        std::process::exit(1)
    });
    let mut c = Counters { fds_baseline: open_fd_count(), ..Default::default() };
    eprintln!("consumer: listening on {} ({}); waiting for a producer", a.socket, drm.path);
    let sock = listener.accept().unwrap_or_else(|e| {
        eprintln!("consumer: accept: {e}");
        std::process::exit(1)
    });

    let mut epochs: Vec<Epoch> = Vec::new();
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let mut producer_gone = false;
    let mut report = Report::default();
    let period_ns = (1e9 / a.pace_hz) as u64;
    let mut next_at = kernel::now_ns();
    let mut begun = false;

    loop {
        // ---- control channel: drain what is queued, never wait ------------------------------
        loop {
            match sock.recv(&mut buf, true) {
                Ok(Some(d)) => {
                    if d.truncated {
                        eprintln!("consumer: truncated datagram ignored");
                        continue;
                    }
                    match Message::decode(&buf[..d.len]) {
                        Ok(Message::Register { producer_epoch, layer_kind, max_in_flight, image_count, register_size: _, name, entries }) => {
                            c.epochs_seen += 1;
                            eprintln!("consumer: REGISTER epoch {producer_epoch} '{name}' layer {} {image_count} images, producer max_in_flight {max_in_flight}", layer_name(layer_kind));
                            let mut fds = d.fds;
                            if fds.is_empty() || max_in_flight < a.max_in_flight || image_count == 0 || image_count > 256 {
                                let _ = sock.send(&Message::RegisterAck { producer_epoch, status: wire::ACK_FAILED, max_in_flight: a.max_in_flight }.encode(), &[]);
                                c.registration_failures += 1;
                                continue;
                            }
                            let memfd = fds.remove(0);
                            let mut ep = Epoch { epoch: producer_epoch, layer_kind, image_count, register: None, images: (0..image_count).map(|_| None).collect(), received: 0, acked: false, current: None, uses: Vec::new(), dying_since: None, last_calibration: None, use_page: None, newest_completed_submit: 0 };
                            match ConsumerRegister::open(memfd.as_raw_fd()) {
                                Ok(r) => ep.register = Some(r),
                                Err(e) => {
                                    eprintln!("consumer: register mapping refused: {e}");
                                    let _ = sock.send(&Message::RegisterAck { producer_epoch, status: wire::ACK_FAILED, max_in_flight: a.max_in_flight }.encode(), &[]);
                                    c.registration_failures += 1;
                                    continue;
                                }
                            }
                            drop(memfd); // the mapping keeps the memory; the fd is not needed
                            if let Err(e) = ep.add_entries(&drm, 0, &entries, fds) {
                                eprintln!("consumer: REGISTER entries refused: {e}");
                                let _ = sock.send(&Message::RegisterAck { producer_epoch, status: wire::ACK_FAILED, max_in_flight: a.max_in_flight }.encode(), &[]);
                                c.registration_failures += 1;
                                continue;
                            }
                            epochs.push(ep);
                        }
                        Ok(Message::RegisterMore { producer_epoch, fd_base, entries }) => {
                            if let Some(ep) = epochs.iter_mut().find(|e| e.epoch == producer_epoch && !e.acked) {
                                if let Err(e) = ep.add_entries(&drm, fd_base as usize, &entries, d.fds) {
                                    eprintln!("consumer: REGISTER_MORE refused: {e}");
                                    let _ = sock.send(&Message::RegisterAck { producer_epoch, status: wire::ACK_FAILED, max_in_flight: a.max_in_flight }.encode(), &[]);
                                    c.registration_failures += 1;
                                }
                            }
                        }
                        Ok(Message::Generation(_)) => c.generation_notifications += 1,
                        Ok(Message::Overrun { dropped_total, .. }) => {
                            c.overrun_reports += 1;
                            c.overrun_dropped_total = dropped_total;
                        }
                        Ok(Message::Goodbye { producer_epoch }) => {
                            c.goodbyes += 1;
                            eprintln!("consumer: GOODBYE epoch {producer_epoch}");
                            for ep in epochs.iter_mut().filter(|e| e.epoch == producer_epoch) {
                                ep.dying_since.get_or_insert(kernel::now_ns());
                            }
                        }
                        Ok(Message::RegisterAck { .. }) => {}
                        Ok(Message::Unknown { ty }) => {
                            c.unknown_ignored += 1;
                            eprintln!("consumer: unknown type {ty} ignored");
                        }
                        Err(DecodeError::FutureVersion(v)) => {
                            eprintln!("consumer: datagram of version {v} > {}: registration failure", wire::VERSION);
                            c.registration_failures += 1;
                            let _ = sock.send(&Message::RegisterAck { producer_epoch: 0, status: wire::ACK_FAILED, max_in_flight: a.max_in_flight }.encode(), &[]);
                        }
                        Err(e) => eprintln!("consumer: malformed datagram ignored: {e:?}"),
                    }
                }
                Ok(None) => {
                    if !producer_gone {
                        producer_gone = true;
                        c.producer_gone = 1;
                        c.fds_at_death = open_fd_count();
                        eprintln!("consumer: producer gone (EOF); composing without the layer from the next pass");
                        let now = kernel::now_ns();
                        for ep in epochs.iter_mut() {
                            ep.dying_since.get_or_insert(now);
                        }
                    }
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("consumer: recv: {e}");
                    break;
                }
            }
        }
        // ---- registration completion: ack, and supersede older epochs (§6) -------------------
        let mut newly_acked: Option<u64> = None;
        for ep in epochs.iter_mut() {
            if !ep.acked && ep.complete() {
                ep.acked = true;
                newly_acked = Some(ep.epoch);
                let page = ConsumerUsePage::create(a.max_in_flight).unwrap_or_else(|e| {
                    eprintln!("consumer: use page: {e}");
                    std::process::exit(1)
                });
                let _ = sock.send(&Message::RegisterAck { producer_epoch: ep.epoch, status: wire::ACK_OK, max_in_flight: a.max_in_flight }.encode(), &[page.fd()]);
                ep.use_page = Some(page);
                c.fds_registered = open_fd_count();
                eprintln!("consumer: epoch {} registered ({} images, layer {}); acked", ep.epoch, ep.image_count, layer_name(ep.layer_kind));
            }
        }
        if let Some(new) = newly_acked {
            let now = kernel::now_ns();
            for ep in epochs.iter_mut().filter(|e| e.epoch < new) {
                if ep.dying_since.is_none() {
                    eprintln!("consumer: epoch {} superseded by {new}", ep.epoch);
                    ep.dying_since = Some(now);
                }
            }
        }

        // ---- the pass (§4/§5): one register read, one fence query, select or keep -----------
        if !begun && epochs.iter().any(|e| e.acked) {
            begun = true;
            let _ = std::io::stderr().write_all(b"PASSES_BEGIN\n");
        }
        let now = kernel::now_ns();
        c.passes += 1;
        let mut continue_pass = true;
        if let Some(ep) = epochs.iter_mut().find(|e| e.acked && e.dying_since.is_none()) {
            let reg = ep.register.as_ref().unwrap();
            match reg.read_latest() {
                None => c.layer_absent += 1,
                Some((slot, rec)) => {
                    let same = ep.current.as_ref().map(|(_, cur)| cur.generation == rec.generation).unwrap_or(false);
                    let in_flight_limit = a.exceed_in_flight.unwrap_or(a.max_in_flight as usize);
                    if !same && ep.pending_uses() >= in_flight_limit {
                        // our own promise (§4: at most max_in_flight generations referenced by
                        // submitted GPU work): keep the current one until a use completes
                        c.fallback_in_flight_full += 1;
                    } else if !same {
                        // the two-flag exchange (register.rs): declare the intent, then check the
                        // producer is not reclaiming this slot and has not already rewritten it
                        let page = ep.use_page.as_ref().unwrap();
                        page.set_intent(rec.generation);
                        if reg.reclaiming() == rec.generation || reg.slot_generation(slot) != rec.generation {
                            page.clear_intent();
                            c.selection_backed_off += 1;
                            if ep.current.is_some() {
                                c.fallback_reused_current += 1;
                            } else {
                                c.layer_absent += 1;
                            }
                            continue_pass = false;
                        }
                        if !continue_pass {
                            // nothing more for this pass
                        } else {
                        // acquire points: one query over the generation's images
                        let refs: Vec<_> = rec.image_refs().collect();
                        let handles: Vec<u32> = refs.iter().filter_map(|r| ep.images.get(r.slot_index as usize).and_then(|i| i.as_ref()).map(|i| i.acquire.handle)).collect();
                        let mut points = vec![0u64; handles.len()];
                        let ok = handles.len() == refs.len() && kernel::query_many(&drm, &handles, &mut points).is_ok() && refs.iter().zip(&points).all(|(r, p)| *p >= r.acquire_point);
                        if ok {
                            if !page.pending_add(rec.generation) {
                                // Refuse before submitting any GPU use. Publishing the use without
                                // its declaration would let the producer reclaim live images.
                                eprintln!("consumer: pending table full — refusing selection");
                                page.clear_intent();
                                c.pending_table_full_refused += 1;
                                if ep.current.is_some() {
                                    c.fallback_reused_current += 1;
                                } else {
                                    c.layer_absent += 1;
                                }
                            } else {
                            // stamps: pixels and record agree, per group's calibration (§8 items 1, 3)
                            for r in &rec.colour.images[..rec.colour.image_count as usize] {
                                let s = stamp_of(ep.images[r.slot_index as usize].as_ref().unwrap());
                                if s.producer_epoch != rec.producer_epoch || s.generation != rec.generation || s.slot_index != r.slot_index {
                                    c.stamp_mismatch += 1;
                                }
                                if s.calibration_ver != rec.colour.calibration_ver {
                                    c.calibration_mismatch += 1;
                                }
                            }
                            for r in &rec.geometry.images[..rec.geometry.image_count as usize] {
                                let s = stamp_of(ep.images[r.slot_index as usize].as_ref().unwrap());
                                if s.producer_epoch != rec.producer_epoch || s.generation != rec.generation || s.slot_index != r.slot_index {
                                    c.stamp_mismatch += 1;
                                }
                                if s.calibration_ver != rec.geometry.calibration_ver {
                                    c.calibration_mismatch += 1;
                                }
                            }
                            if let Some(prev) = ep.last_calibration {
                                if prev != rec.colour.calibration_ver {
                                    c.calibration_changes_seen += 1;
                                }
                            }
                            ep.last_calibration = Some(rec.colour.calibration_ver);
                            if let Some((_, cur)) = &ep.current {
                                c.generations_skipped += rec.generation.saturating_sub(cur.generation + 1);
                            }
                            // submit the fake GPU use: releases later, per image
                            let n = c.uses_submitted;
                            let images = refs
                                .iter()
                                .enumerate()
                                .map(|(k, r)| {
                                    let mut at = now + a.gpu_ms * 1_000_000;
                                    if a.swap && n % 2 == 0 && k == 1 {
                                        at += 3 * a.gpu_ms * 1_000_000; // this image of an even use releases after the next use's
                                    }
                                    (r.slot_index, r.release_point, at, false)
                                })
                                .collect();
                            ep.uses.push(Use { generation: rec.generation, images, submitted_at: now });
                            page.clear_intent();
                            c.uses_submitted += 1;
                            c.selections += 1;
                            c.last_generation = rec.generation;
                            ep.current = Some((slot, rec));
                            }
                        } else {
                            page.clear_intent();
                            if ep.current.is_some() {
                                c.fallback_reused_current += 1; // §5 rule 1: unsignalled acquire ⇒ reuse current
                            } else {
                                c.layer_absent += 1;
                            }
                        }
                        }
                    }
                }
            }
        } else {
            c.layer_absent += 1;
        }

        // ---- fake GPU completion: signal release points whose time has come ------------------
        for ep in epochs.iter_mut() {
            let hold_until = ep.dying_since.map(|t| t + a.hold_release_ms * 1_000_000);
            let mut newest = ep.newest_completed_submit;
            for u in ep.uses.iter_mut() {
                for (img, point, at, done) in u.images.iter_mut() {
                    if *done || *at > now || hold_until.map(|h| now < h).unwrap_or(false) {
                        continue;
                    }
                    if let Some(Some(image)) = ep.images.get(*img as usize) {
                        // the producer must not have touched this image while our use was pending
                        let s = stamp_of(image);
                        if s.generation != u.generation {
                            c.overwritten_while_in_use += 1;
                        }
                        if let Err(e) = image.release.signal(*point) {
                            eprintln!("consumer: release signal failed: {e}");
                        }
                    }
                    *done = true;
                    if u.submitted_at < newest {
                        c.out_of_order_releases += 1; // an image of an older use released after a newer use's
                    }
                    newest = newest.max(u.submitted_at);
                }
            }
            ep.newest_completed_submit = newest;
            let before = ep.uses.len();
            for u in ep.uses.iter().filter(|u| u.images.iter().all(|i| i.3)) {
                if let Some(p) = &ep.use_page {
                    p.pending_remove(u.generation); // the declaration is withdrawn once the fences say so
                }
            }
            ep.uses.retain(|u| u.images.iter().any(|i| !i.3));
            c.uses_completed += (before - ep.uses.len()) as u64;
            c.max_pending_uses = c.max_pending_uses.max(ep.pending_uses() as u64);
        }

        // ---- retirement (§6): a dying epoch goes only when every use completed --------------
        let mut retired = false;
        epochs.retain(|ep| {
            if ep.dying_since.is_some() && ep.uses.is_empty() {
                c.retire_delay_ms = (now - ep.dying_since.unwrap()) / 1_000_000;
                eprintln!("consumer: epoch {} retired after {} ms: images unmapped, timelines destroyed, fds closed", ep.epoch, c.retire_delay_ms);
                retired = true;
                false
            } else {
                true
            }
        });
        if retired {
            c.epochs_retired += 1;
            c.fds_after_retire = open_fd_count();
        }

        // ---- exit conditions -----------------------------------------------------------------
        if let Some(n) = a.passes {
            if c.passes >= n {
                break;
            }
        } else if (producer_gone || c.goodbyes > 0) && epochs.is_empty() {
            break;
        }

        // ---- pace: the harness's stand-in for xrWaitFrame, between passes -------------------
        next_at += period_ns;
        let now2 = kernel::now_ns();
        if next_at > now2 {
            if a.spin {
                while kernel::now_ns() < next_at {
                    std::hint::spin_loop();
                }
            } else {
                let d = next_at - now2;
                let ts = libc::timespec { tv_sec: (d / 1_000_000_000) as _, tv_nsec: (d % 1_000_000_000) as _ };
                // SAFETY: plain nanosleep, outside the pass.
                unsafe { libc::nanosleep(&ts, std::ptr::null_mut()) };
            }
        } else if now2 - next_at > period_ns * 10 {
            next_at = now2;
        }
    }
    if begun {
        let _ = std::io::stderr().write_all(b"PASSES_END\n");
    }
    drop(epochs);
    let fds_end = open_fd_count();

    report.set("passes", c.passes);
    report.set("selections", c.selections);
    report.set("fallback_reused_current", c.fallback_reused_current);
    report.set("layer_absent", c.layer_absent);
    report.set("generations_skipped", c.generations_skipped);
    report.set("stamp_mismatch", c.stamp_mismatch);
    report.set("calibration_mismatch", c.calibration_mismatch);
    report.set("calibration_changes_seen", c.calibration_changes_seen);
    report.set("overwritten_while_in_use", c.overwritten_while_in_use);
    report.set("selection_backed_off", c.selection_backed_off);
    report.set("uses_submitted", c.uses_submitted);
    report.set("uses_completed", c.uses_completed);
    report.set("out_of_order_releases", c.out_of_order_releases);
    report.set("max_pending_uses", c.max_pending_uses);
    report.set("fallback_in_flight_full", c.fallback_in_flight_full);
    report.set("pending_table_full_refused", c.pending_table_full_refused);
    report.set("overrun_reports", c.overrun_reports);
    report.set("overrun_dropped_total", c.overrun_dropped_total);
    report.set("generation_notifications", c.generation_notifications);
    report.set("unknown_ignored", c.unknown_ignored);
    report.set("registration_failures", c.registration_failures);
    report.set("epochs_seen", c.epochs_seen);
    report.set("epochs_retired", c.epochs_retired);
    report.set("goodbyes", c.goodbyes);
    report.set("producer_gone", c.producer_gone);
    report.set("last_generation", c.last_generation);
    report.set("fds_baseline", c.fds_baseline);
    report.set("fds_registered", c.fds_registered);
    report.set("fds_at_death", c.fds_at_death);
    report.set("fds_after_retire", c.fds_after_retire);
    report.set("fds_end", fds_end);
    report.set("retire_delay_ms", c.retire_delay_ms);
    report.set("nonzero_timeout_waits", NONZERO_TIMEOUT_WAITS.load(Ordering::Relaxed));
    if let Some(p) = &a.report {
        report.write(p);
    }
    eprint!("{}", report.render());
}
