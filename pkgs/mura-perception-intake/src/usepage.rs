//! The consumer's use page: one page-sized memfd the consumer owns and writes, the producer maps
//! read-only (the mirror of the register's ownership). It carries the declaration the fences
//! cannot: which generations the consumer has submitted uses of (`pending`, at most
//! `max_in_flight` + a margin), and the one it is taking this instant (`intent`) — the
//! consumer's half of the two-flag agreement described in `register` (Dekker: store `intent`,
//! then read `reclaiming`; the producer stores `reclaiming`, then reads `intent`/`pending`).
//! The fd travels once, in REGISTER_ACK (§7); one page per producer epoch.
//!
//! Layout: magic u64 | version u32 | max_in_flight u32 | intent AtomicU64 | pending[16] AtomicU64
//! (0 = empty; generations start at 1).

use crate::kernel::{self, Mapping};
use std::io;
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAGIC: u64 = 0x4553_5545_4e49_5053; // "SPINUSE" + pad
pub const VERSION: u32 = 1;
pub const PENDING_SLOTS: usize = 16;
pub const NONE: u64 = u64::MAX;

const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 8;
const OFF_MAX_IN_FLIGHT: usize = 12;
const OFF_INTENT: usize = 16;
const OFF_PENDING: usize = 24;

fn atomic(base: *mut u8, off: usize) -> &'static AtomicU64 {
    // SAFETY: 8-aligned offsets inside a page-aligned mapping that outlives every use through
    // the owning struct; AtomicU64 over shared memory is the protocol's contract.
    unsafe { &*(base.add(off) as *const AtomicU64) }
}

/// The consumer's side: owns the page and the memfd.
pub struct ConsumerUsePage {
    fd: OwnedFd,
    map: Mapping,
}

impl ConsumerUsePage {
    pub fn create(max_in_flight: u32) -> io::Result<ConsumerUsePage> {
        let size = kernel::page_round(OFF_PENDING + PENDING_SLOTS * 8);
        let fd = kernel::memfd("mura-intake-use", size)?;
        let mut map = Mapping::map(fd.as_raw_fd(), size, true)?;
        let b = map.bytes_mut();
        b[OFF_MAGIC..OFF_MAGIC + 8].copy_from_slice(&MAGIC.to_le_bytes());
        b[OFF_VERSION..OFF_VERSION + 4].copy_from_slice(&VERSION.to_le_bytes());
        b[OFF_MAX_IN_FLIGHT..OFF_MAX_IN_FLIGHT + 4].copy_from_slice(&max_in_flight.to_le_bytes());
        atomic(map.as_ptr(), OFF_INTENT).store(NONE, Ordering::SeqCst);
        for i in 0..PENDING_SLOTS {
            atomic(map.as_ptr(), OFF_PENDING + i * 8).store(0, Ordering::SeqCst);
        }
        Ok(ConsumerUsePage { fd, map })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// "I am about to take `generation`" — stored before the producer's `reclaiming` is read.
    pub fn set_intent(&self, generation: u64) {
        atomic(self.map.as_ptr(), OFF_INTENT).store(generation, Ordering::SeqCst);
    }
    pub fn clear_intent(&self) {
        atomic(self.map.as_ptr(), OFF_INTENT).store(NONE, Ordering::SeqCst);
    }

    /// Declare a submitted use. `false` if the table is full (the consumer exceeded its own
    /// max_in_flight — a bug on its side, never the producer's problem).
    pub fn pending_add(&self, generation: u64) -> bool {
        for i in 0..PENDING_SLOTS {
            let a = atomic(self.map.as_ptr(), OFF_PENDING + i * 8);
            if a.compare_exchange(0, generation, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return true;
            }
        }
        false
    }

    /// The use completed (its release points are signalled): withdraw the declaration.
    pub fn pending_remove(&self, generation: u64) {
        for i in 0..PENDING_SLOTS {
            let a = atomic(self.map.as_ptr(), OFF_PENDING + i * 8);
            let _ = a.compare_exchange(generation, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
}

/// The producer's side: a read-only mapping of the consumer's page.
pub struct ProducerUsePage {
    map: Mapping,
    pub max_in_flight: u32,
}

impl ProducerUsePage {
    pub fn open(fd: RawFd) -> io::Result<ProducerUsePage> {
        let size = kernel::page_round(OFF_PENDING + PENDING_SLOTS * 8);
        let map = Mapping::map(fd, size, false)?;
        let b = map.bytes();
        let magic = u64::from_le_bytes(b[OFF_MAGIC..OFF_MAGIC + 8].try_into().unwrap());
        let version = u32::from_le_bytes(b[OFF_VERSION..OFF_VERSION + 4].try_into().unwrap());
        if magic != MAGIC || version != VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("use page: bad magic/version {magic:#x}/{version}")));
        }
        let max_in_flight = u32::from_le_bytes(b[OFF_MAX_IN_FLIGHT..OFF_MAX_IN_FLIGHT + 4].try_into().unwrap());
        Ok(ProducerUsePage { map, max_in_flight })
    }

    pub fn intent(&self) -> u64 {
        atomic(self.map.as_ptr(), OFF_INTENT).load(Ordering::SeqCst)
    }

    pub fn pending_contains(&self, generation: u64) -> bool {
        (0..PENDING_SLOTS).any(|i| atomic(self.map.as_ptr(), OFF_PENDING + i * 8).load(Ordering::SeqCst) == generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_round_trip_through_the_page() {
        let c = ConsumerUsePage::create(2).unwrap();
        let p = ProducerUsePage::open(c.fd()).unwrap();
        assert_eq!(p.max_in_flight, 2);
        assert_eq!(p.intent(), NONE);
        c.set_intent(7);
        assert_eq!(p.intent(), 7);
        assert!(c.pending_add(7));
        assert!(p.pending_contains(7));
        c.clear_intent();
        c.pending_remove(7);
        assert!(!p.pending_contains(7));
        assert_eq!(p.intent(), NONE);
    }
}
