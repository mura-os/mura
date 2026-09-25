//! The adjacent-process encoding (§7): versioned, fixed-layout little-endian records over
//! SOCK_SEQPACKET — `magic "SPIN" | version u32 | byte_length u32 | type u32 | body`.
//!
//! Types: REGISTER (identity + the first image entries; fds via SCM_RIGHTS: the register memfd
//! first, then dmabuf/acquire/release per entry), REGISTER_MORE (further entries, `fd_base` =
//! the index of the first entry carried), REGISTER_ACK (status + the consumer's `max_in_flight`;
//! on success one fd: the consumer's use page — `usepage`),
//! GENERATION (the §2 record, a notification — the register is authoritative), OVERRUN (count),
//! GOODBYE. Unknown types with `version ≤ VERSION` are ignored; a higher version is a
//! registration failure. Image data never travels in a datagram.

use crate::record::{Record, RECORD_SIZE};

pub const MAGIC: [u8; 4] = *b"SPIN";
pub const VERSION: u32 = 1;
pub const HEADER: usize = 16;

pub const T_REGISTER: u32 = 1;
pub const T_REGISTER_MORE: u32 = 2;
pub const T_REGISTER_ACK: u32 = 3;
pub const T_GENERATION: u32 = 4;
pub const T_OVERRUN: u32 = 5;
pub const T_GOODBYE: u32 = 6;

pub const ACK_OK: u32 = 0;
pub const ACK_FAILED: u32 = 1;

/// Entries per datagram so that 1 (memfd) + 3 × entries ≤ 16 fds in REGISTER, 3 × entries ≤ 16
/// in REGISTER_MORE.
pub const ENTRIES_PER_REGISTER: usize = 5;
pub const ENTRIES_PER_MORE: usize = 5;
pub const FDS_PER_ENTRY: usize = 3;
pub const MAX_NAME: usize = 32;

/// One row of the registered image table (§3). fds are positional in the carrying datagram.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageEntry {
    pub slot_index: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub offset: u32,
    pub size: u32,
    pub usage: u32,
}
pub const ENTRY_SIZE: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Register { producer_epoch: u64, layer_kind: u32, max_in_flight: u32, image_count: u32, register_size: u32, name: String, entries: Vec<ImageEntry> },
    RegisterMore { producer_epoch: u64, fd_base: u32, entries: Vec<ImageEntry> },
    RegisterAck { producer_epoch: u64, status: u32, max_in_flight: u32 },
    Generation(Record),
    Overrun { producer_epoch: u64, dropped_total: u64 },
    Goodbye { producer_epoch: u64 },
    /// A well-formed record of a type this version does not know: ignored by the peer.
    Unknown { ty: u32 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    Short,
    BadMagic,
    /// `version` above ours: a registration failure (§7).
    FutureVersion(u32),
    LengthMismatch,
    Malformed,
}

struct W(Vec<u8>);
impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn entry(&mut self, e: &ImageEntry) {
        self.u32(e.slot_index);
        self.u32(e.fourcc);
        self.u64(e.modifier);
        self.u32(e.width);
        self.u32(e.height);
        self.u32(e.stride);
        self.u32(e.offset);
        self.u32(e.size);
        self.u32(e.usage);
    }
}

struct R<'a>(&'a [u8], usize);
impl R<'_> {
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.0.get(self.1..self.1 + 4).ok_or(DecodeError::Malformed)?;
        self.1 += 4;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.0.get(self.1..self.1 + 8).ok_or(DecodeError::Malformed)?;
        self.1 += 8;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn entry(&mut self) -> Result<ImageEntry, DecodeError> {
        Ok(ImageEntry {
            slot_index: self.u32()?,
            fourcc: self.u32()?,
            modifier: self.u64()?,
            width: self.u32()?,
            height: self.u32()?,
            stride: self.u32()?,
            offset: self.u32()?,
            size: self.u32()?,
            usage: self.u32()?,
        })
    }
    fn entries(&mut self, n: u32) -> Result<Vec<ImageEntry>, DecodeError> {
        if n as usize > 16 {
            return Err(DecodeError::Malformed);
        }
        (0..n).map(|_| self.entry()).collect()
    }
}

fn frame(ty: u32, version: u32, body: &[u8]) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(HEADER + body.len()));
    w.0.extend_from_slice(&MAGIC);
    w.u32(version);
    w.u32((HEADER + body.len()) as u32);
    w.u32(ty);
    w.0.extend_from_slice(body);
    w.0
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        self.encode_with_version(VERSION)
    }

    /// `version` other than ours exists for the harness (a future-version probe).
    pub fn encode_with_version(&self, version: u32) -> Vec<u8> {
        let mut w = W(Vec::new());
        let ty = match self {
            Message::Register { producer_epoch, layer_kind, max_in_flight, image_count, register_size, name, entries } => {
                w.u64(*producer_epoch);
                w.u32(*layer_kind);
                w.u32(*max_in_flight);
                w.u32(*image_count);
                w.u32(*register_size);
                let nb = name.as_bytes();
                let n = nb.len().min(MAX_NAME);
                w.u32(n as u32);
                let mut name_buf = [0u8; MAX_NAME];
                name_buf[..n].copy_from_slice(&nb[..n]);
                w.0.extend_from_slice(&name_buf);
                w.u32(entries.len() as u32);
                for e in entries {
                    w.entry(e);
                }
                T_REGISTER
            }
            Message::RegisterMore { producer_epoch, fd_base, entries } => {
                w.u64(*producer_epoch);
                w.u32(*fd_base);
                w.u32(entries.len() as u32);
                for e in entries {
                    w.entry(e);
                }
                T_REGISTER_MORE
            }
            Message::RegisterAck { producer_epoch, status, max_in_flight } => {
                w.u64(*producer_epoch);
                w.u32(*status);
                w.u32(*max_in_flight);
                T_REGISTER_ACK
            }
            Message::Generation(rec) => {
                let mut b = [0u8; RECORD_SIZE];
                rec.encode(&mut b);
                w.0.extend_from_slice(&b);
                T_GENERATION
            }
            Message::Overrun { producer_epoch, dropped_total } => {
                w.u64(*producer_epoch);
                w.u64(*dropped_total);
                T_OVERRUN
            }
            Message::Goodbye { producer_epoch } => {
                w.u64(*producer_epoch);
                T_GOODBYE
            }
            Message::Unknown { ty } => *ty,
        };
        frame(ty, version, &w.0)
    }

    pub fn decode(b: &[u8]) -> Result<Message, DecodeError> {
        if b.len() < HEADER {
            return Err(DecodeError::Short);
        }
        if b[..4] != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let mut r = R(b, 4);
        let version = r.u32()?;
        let len = r.u32()?;
        let ty = r.u32()?;
        if version > VERSION {
            return Err(DecodeError::FutureVersion(version));
        }
        if len as usize != b.len() {
            return Err(DecodeError::LengthMismatch);
        }
        Ok(match ty {
            T_REGISTER => {
                let producer_epoch = r.u64()?;
                let layer_kind = r.u32()?;
                let max_in_flight = r.u32()?;
                let image_count = r.u32()?;
                let register_size = r.u32()?;
                let name_len = r.u32()? as usize;
                let name_b = b.get(r.1..r.1 + MAX_NAME).ok_or(DecodeError::Malformed)?;
                r.1 += MAX_NAME;
                let name = String::from_utf8_lossy(&name_b[..name_len.min(MAX_NAME)]).into_owned();
                let n = r.u32()?;
                Message::Register { producer_epoch, layer_kind, max_in_flight, image_count, register_size, name, entries: r.entries(n)? }
            }
            T_REGISTER_MORE => {
                let producer_epoch = r.u64()?;
                let fd_base = r.u32()?;
                let n = r.u32()?;
                Message::RegisterMore { producer_epoch, fd_base, entries: r.entries(n)? }
            }
            T_REGISTER_ACK => Message::RegisterAck { producer_epoch: r.u64()?, status: r.u32()?, max_in_flight: r.u32()? },
            T_GENERATION => Message::Generation(Record::decode(&b[HEADER..]).ok_or(DecodeError::Malformed)?),
            T_OVERRUN => Message::Overrun { producer_epoch: r.u64()?, dropped_total: r.u64()? },
            T_GOODBYE => Message::Goodbye { producer_epoch: r.u64()? },
            other => Message::Unknown { ty: other },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_round_trips_with_entries_and_name() {
        let m = Message::Register {
            producer_epoch: 3,
            layer_kind: 1,
            max_in_flight: 2,
            image_count: 32,
            register_size: 8192,
            name: "fake-producer".into(),
            entries: vec![ImageEntry { slot_index: 0, fourcc: 0x3432_4241, modifier: 0, width: 64, height: 64, stride: 256, offset: 0, size: 16384, usage: 1 }; ENTRIES_PER_REGISTER],
        };
        let b = m.encode();
        assert_eq!(Message::decode(&b), Ok(m));
        assert!(b.len() <= crate::seqpacket::MAX_DATAGRAM);
    }

    #[test]
    fn generation_is_one_record() {
        let rec = Record { generation: 5, ..Default::default() };
        let b = Message::Generation(rec).encode();
        assert_eq!(b.len(), HEADER + RECORD_SIZE);
        assert_eq!(Message::decode(&b), Ok(Message::Generation(rec)));
    }

    #[test]
    fn framing_rules() {
        assert_eq!(Message::decode(b"SPI"), Err(DecodeError::Short));
        assert_eq!(Message::decode(&Message::Goodbye { producer_epoch: 1 }.encode_with_version(VERSION + 1)), Err(DecodeError::FutureVersion(VERSION + 1)));
        let mut b = Message::Goodbye { producer_epoch: 1 }.encode();
        b.push(0);
        assert_eq!(Message::decode(&b), Err(DecodeError::LengthMismatch));
        let mut bad = Message::Goodbye { producer_epoch: 1 }.encode();
        bad[0] = b'X';
        assert_eq!(Message::decode(&bad), Err(DecodeError::BadMagic));
        // an unknown type at our version decodes as Unknown → ignored by the peer
        assert_eq!(Message::decode(&Message::Unknown { ty: 99 }.encode()), Ok(Message::Unknown { ty: 99 }));
    }
}
