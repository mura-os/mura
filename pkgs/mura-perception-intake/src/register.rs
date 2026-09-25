//! The GPU-safe latest-signalled register (§4): a producer-owned memfd the consumer maps
//! read-only. `pool = 2 + max_in_flight` fixed slots of `RECORD_SIZE`; publication is a full
//! slot write followed by one atomic store of the latest index with release ordering; the
//! consumer performs one acquire-ordered load and reads that slot — no retry, no lock. Slots
//! are rewritten only after reclamation, so a torn read is impossible by construction.
//!
//! Reclamation needs one thing the fences cannot say: whether the consumer ever *used* a
//! generation. A generation superseded before the consumer looked at it has no submitted use and
//! no release point that will ever signal; without a declaration from the consumer the producer
//! could not tell it from one in flight and the pool would wedge (the §8 harness found exactly
//! this under a slow consumer). So the consumer declares its uses on its own page (`usepage`) and
//! the two sides agree on a slot with two flags — the producer's `reclaiming` here, the
//! consumer's `intent` there — each stored before the other's is read (SeqCst), so at least one
//! side sees the other and backs off without waiting: Dekker's shape, no lock, no retry loop.
//!
//! Layout: `Header` (64 bytes) then `slot_count × RECORD_SIZE`.

use crate::kernel::{self, Mapping};
use crate::record::{Record, RECORD_SIZE};
use std::io;
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAGIC: u64 = 0x5245_5245_4e49_5053; // "SPINRERE" — the register's own magic
pub const VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 64;
pub const NO_LATEST: u64 = u64::MAX;

const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 8;
const OFF_LAYER: usize = 12;
const OFF_SLOT_COUNT: usize = 16;
const OFF_SLOT_SIZE: usize = 20;
const OFF_EPOCH: usize = 24;
const OFF_LATEST: usize = 32; // AtomicU64: slot index of the latest complete generation
const OFF_RECLAIMING: usize = 40; // AtomicU64: generation the producer is about to reclaim (NO_LATEST = none)

pub fn size_for(slot_count: u32) -> usize {
    kernel::page_round(HEADER_SIZE + slot_count as usize * RECORD_SIZE)
}

fn latest_atomic(base: *mut u8) -> &'static AtomicU64 {
    // SAFETY: OFF_LATEST is 8-aligned inside a page-aligned mapping that outlives every use
    // through the owning struct; AtomicU64 over shared memory is the protocol's contract.
    unsafe { &*(base.add(OFF_LATEST) as *const AtomicU64) }
}
fn reclaiming_atomic(base: *mut u8) -> &'static AtomicU64 {
    // SAFETY: as above.
    unsafe { &*(base.add(OFF_RECLAIMING) as *const AtomicU64) }
}

/// The producer's side: owns the memfd, writes slots, publishes.
pub struct ProducerRegister {
    fd: OwnedFd,
    map: Mapping,
    pub slot_count: u32,
}

impl ProducerRegister {
    pub fn create(layer_kind: u32, producer_epoch: u64, slot_count: u32) -> io::Result<ProducerRegister> {
        let size = size_for(slot_count);
        let fd = kernel::memfd("mura-intake-register", size)?;
        let mut map = Mapping::map(fd.as_raw_fd(), size, true)?;
        let b = map.bytes_mut();
        b[OFF_MAGIC..OFF_MAGIC + 8].copy_from_slice(&MAGIC.to_le_bytes());
        b[OFF_VERSION..OFF_VERSION + 4].copy_from_slice(&VERSION.to_le_bytes());
        b[OFF_LAYER..OFF_LAYER + 4].copy_from_slice(&layer_kind.to_le_bytes());
        b[OFF_SLOT_COUNT..OFF_SLOT_COUNT + 4].copy_from_slice(&slot_count.to_le_bytes());
        b[OFF_SLOT_SIZE..OFF_SLOT_SIZE + 4].copy_from_slice(&(RECORD_SIZE as u32).to_le_bytes());
        b[OFF_EPOCH..OFF_EPOCH + 8].copy_from_slice(&producer_epoch.to_le_bytes());
        latest_atomic(map.as_ptr()).store(NO_LATEST, Ordering::Release);
        reclaiming_atomic(map.as_ptr()).store(NO_LATEST, Ordering::Release);
        Ok(ProducerRegister { fd, map, slot_count })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    /// Write a complete record into `slot` (which must be reclaimed — the caller's `Pool`
    /// guarantees it), then make it the latest. The two steps are the whole publish (§4).
    pub fn publish(&mut self, slot: u32, record: &Record) {
        assert!(slot < self.slot_count);
        let mut buf = [0u8; RECORD_SIZE];
        record.encode(&mut buf);
        let off = HEADER_SIZE + slot as usize * RECORD_SIZE;
        self.map.bytes_mut()[off..off + RECORD_SIZE].copy_from_slice(&buf);
        latest_atomic(self.map.as_ptr()).store(slot as u64, Ordering::Release);
    }

    pub fn latest(&self) -> u64 {
        latest_atomic(self.map.as_ptr()).load(Ordering::Acquire)
    }

    /// Announce the generation about to be reclaimed (before reading the consumer's page).
    pub fn set_reclaiming(&self, generation: u64) {
        reclaiming_atomic(self.map.as_ptr()).store(generation, Ordering::SeqCst);
    }
    pub fn clear_reclaiming(&self) {
        reclaiming_atomic(self.map.as_ptr()).store(NO_LATEST, Ordering::SeqCst);
    }
}

/// The consumer's side: a read-only mapping of the producer's memfd.
pub struct ConsumerRegister {
    map: Mapping,
    pub layer_kind: u32,
    pub slot_count: u32,
    pub producer_epoch: u64,
}

impl ConsumerRegister {
    pub fn open(fd: RawFd) -> io::Result<ConsumerRegister> {
        // the header first, to learn the size
        let head = Mapping::map(fd, kernel::page_round(HEADER_SIZE), false)?;
        let b = head.bytes();
        let magic = u64::from_le_bytes(b[OFF_MAGIC..OFF_MAGIC + 8].try_into().unwrap());
        let version = u32::from_le_bytes(b[OFF_VERSION..OFF_VERSION + 4].try_into().unwrap());
        if magic != MAGIC || version != VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("register: bad magic/version {magic:#x}/{version}")));
        }
        let layer_kind = u32::from_le_bytes(b[OFF_LAYER..OFF_LAYER + 4].try_into().unwrap());
        let slot_count = u32::from_le_bytes(b[OFF_SLOT_COUNT..OFF_SLOT_COUNT + 4].try_into().unwrap());
        let slot_size = u32::from_le_bytes(b[OFF_SLOT_SIZE..OFF_SLOT_SIZE + 4].try_into().unwrap());
        let producer_epoch = u64::from_le_bytes(b[OFF_EPOCH..OFF_EPOCH + 8].try_into().unwrap());
        if slot_size as usize != RECORD_SIZE || slot_count == 0 || slot_count > 64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("register: slot_size {slot_size} slot_count {slot_count}")));
        }
        drop(head);
        let map = Mapping::map(fd, size_for(slot_count), false)?;
        Ok(ConsumerRegister { map, layer_kind, slot_count, producer_epoch })
    }

    /// The one read per pass (§4/§5): the latest index with acquire ordering, then that slot.
    /// `None` when nothing has been published yet.
    pub fn read_latest(&self) -> Option<(u32, Record)> {
        let idx = latest_atomic(self.map.as_ptr()).load(Ordering::Acquire);
        if idx == NO_LATEST || idx >= self.slot_count as u64 {
            return None;
        }
        let off = HEADER_SIZE + idx as usize * RECORD_SIZE;
        let rec = Record::decode(&self.map.bytes()[off..off + RECORD_SIZE])?;
        Some((idx as u32, rec))
    }

    /// The generation the producer is reclaiming right now (read after storing our intent).
    pub fn reclaiming(&self) -> u64 {
        reclaiming_atomic(self.map.as_ptr()).load(Ordering::SeqCst)
    }

    /// The generation number a slot holds now — the consumer's re-validation after the
    /// two-flag exchange (a slot rewritten between its read and its intent shows here).
    pub fn slot_generation(&self, slot: u32) -> u64 {
        let off = HEADER_SIZE + slot as usize * RECORD_SIZE + 8;
        // SAFETY: 8-aligned offset inside the mapping; a relaxed-free SeqCst load pairs with the
        // producer's clear of `reclaiming` after the write.
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }.load(Ordering::SeqCst)
    }
}

/// What the consumer's page says about a generation the producer wants to reclaim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declared {
    /// Never used and not being taken: reusable regardless of fences.
    Free,
    /// A submitted use exists (or existed): reusable once its release points signalled.
    Pending,
    /// Being taken this instant: back off (try another slot, or drop).
    Intent,
}

/// The producer's slot accounting (§4.3): which slot holds which generation and which image
/// release points it depends on. Pure state; the fence values come from the caller.
pub struct Pool {
    slots: Vec<Option<Record>>,
    latest: Option<u32>,
    /// Slots freed by signalled release points (not counting never-used ones).
    pub reclaimed: u64,
}

impl Pool {
    pub fn new(slot_count: u32) -> Pool {
        Pool { slots: vec![None; slot_count as usize], latest: None, reclaimed: 0 }
    }

    /// A reusable slot, if any: never the latest; an empty one; or one whose generation the
    /// consumer declares `Free` (never used), or `Pending` with every release point of every
    /// image signalled (`signalled(image_slot) -> last signalled point`). `declare(generation)`
    /// is the caller's Dekker step: store `reclaiming`, then read the consumer's page. The
    /// consumer moving on is never a release condition — a declared use is released by its
    /// fences only (§4).
    pub fn reclaim(&mut self, signalled: impl Fn(u32) -> u64, mut declare: impl FnMut(u64) -> Declared) -> Option<u32> {
        for (i, s) in self.slots.iter_mut().enumerate() {
            if self.latest == Some(i as u32) {
                continue;
            }
            match s {
                None => return Some(i as u32),
                Some(rec) => {
                    let ok = match declare(rec.generation) {
                        Declared::Free => true,
                        Declared::Pending => rec.image_refs().all(|r| signalled(r.slot_index) >= r.release_point),
                        Declared::Intent => false,
                    };
                    if ok {
                        *s = None;
                        self.reclaimed += 1;
                        return Some(i as u32);
                    }
                }
            }
        }
        None
    }

    pub fn set(&mut self, slot: u32, record: Record) {
        self.slots[slot as usize] = Some(record);
        self.latest = Some(slot);
    }

    pub fn latest(&self) -> Option<u32> {
        self.latest
    }

    pub fn held(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::ImageRef;
    use std::collections::HashMap;

    fn rec(gen: u64, images: &[(u32, u64)]) -> Record {
        let mut r = Record { generation: gen, ..Default::default() };
        r.colour.image_count = images.len() as u32;
        for (i, (slot, point)) in images.iter().enumerate() {
            r.colour.images[i] = ImageRef { slot_index: *slot, acquire_point: *point, release_point: *point };
        }
        r
    }

    #[test]
    fn pool_never_reclaims_latest_and_honours_per_image_points_of_declared_uses() {
        let mut pool = Pool::new(3);
        let mut sig: HashMap<u32, u64> = HashMap::new();
        let pending = |_g: u64| Declared::Pending; // the consumer used everything
        let s0 = pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending).unwrap();
        pool.set(s0, rec(1, &[(0, 1), (1, 1)]));
        let s1 = pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending).unwrap();
        pool.set(s1, rec(2, &[(2, 2), (3, 2)]));
        let s2 = pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending).unwrap();
        pool.set(s2, rec(3, &[(4, 3), (5, 3)]));
        // full: nothing released → overrun-drop
        assert_eq!(pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending), None);
        // one image of gen 1 released, the other not → still held (per image, never a shortcut)
        sig.insert(0, 1);
        assert_eq!(pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending), None);
        sig.insert(1, 1);
        assert_eq!(pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending), Some(s0));
        // the latest (gen 3) is never reclaimed even if its points read as signalled
        sig.insert(4, 3);
        sig.insert(5, 3);
        pool.set(s0, rec(4, &[(0, 4), (1, 4)]));
        let r = pool.reclaim(|i| *sig.get(&i).unwrap_or(&0), pending);
        assert_ne!(r, Some(s0));
        assert_eq!(r, Some(s2)); // gen 3's slot, released, and no longer latest
    }

    #[test]
    fn skipped_generations_are_free_and_an_intent_backs_off() {
        // the consumer looked only at gen 3 (in flight, unreleased) and is taking gen 4 right now
        let mut pool = Pool::new(5);
        let declare = |g: u64| match g {
            3 => Declared::Pending,
            4 => Declared::Intent,
            _ => Declared::Free,
        };
        for g in 1..=5 {
            // fill through the empty slots (everything declared pending while filling)
            let s = pool.reclaim(|_| 0, |_| Declared::Pending).unwrap();
            assert_eq!(s, g as u32 - 1);
            pool.set(s, rec(g, &[(g as u32, g)]));
        }
        // gen 5 latest; 1 and 2 never used → free regardless of fences; 3 pending & unreleased; 4 intent
        assert_eq!(pool.reclaim(|_| 0, declare), Some(0));
        pool.set(0, rec(6, &[(0, 6)]));
        assert_eq!(pool.reclaim(|_| 0, declare), Some(1));
        pool.set(1, rec(7, &[(1, 7)]));
        // the consumer now also holds gen 6: 3 and 6 fenced and unreleased, 4 being taken,
        // 7 latest — gen 5, superseded and never used, is the one that is free
        let declare2 = |g: u64| match g {
            3 | 6 => Declared::Pending,
            4 => Declared::Intent,
            _ => Declared::Free,
        };
        assert_eq!(pool.reclaim(|_| 0, declare2), Some(4));
        pool.set(4, rec(8, &[(4, 8)]));
        // 3 fenced and unreleased, 4 being taken, 6/7 would be free — but if the consumer used
        // everything and released nothing, the pool is full: overrun-drop, never a wait
        assert_eq!(pool.reclaim(|_| 0, |_| Declared::Pending), None);
    }
}
