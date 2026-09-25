//! The generation record (§2): one publishable generation of one layer kind, dual-rate — the
//! colour group and the geometry group each carry their own timestamp, pose and calibration
//! reference. Fixed layout, little-endian, ≤ 6 images per group (stereo × 3 kinds), so a record
//! always fits `RECORD_SIZE` bytes — the slot size of the register (§4) and the body of a
//! `GENERATION` datagram (§7). Device identity, formats, timelines and distortion maps are
//! registration data (§3), not here.

pub const MAX_IMAGES: usize = 6;
pub const RECORD_SIZE: usize = 1024;

pub const FLAG_COMPLETE: u32 = 1;
pub const FLAG_DEGRADED: u32 = 2;
pub const FLAG_FABRICATED_CONFIDENCE: u32 = 4;

/// One image the generation references: a slot of the registered image table and the points
/// on that image's own acquire/release timelines (never a shared release timeline —
/// research/32 §2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageRef {
    pub slot_index: u32,
    pub acquire_point: u64,
    pub release_point: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColourGroup {
    pub t_colour_ns: u64,
    pub pose_colour: [f32; 7],
    pub calibration_ver: u32,
    pub colour_space: u32,
    pub exposure_us: u32,
    pub gain: f32,
    pub distortion_ref: u32,
    pub image_count: u32,
    pub images: [ImageRef; MAX_IMAGES],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeometryGroup {
    pub t_geometry_ns: u64,
    pub pose_geometry: [f32; 7],
    pub calibration_ver: u32,
    pub t_geom_to_colour: [f32; 16],
    pub depth_fourcc: u32,
    pub depth_params: u64,
    pub near_m: f32,
    pub far_m: f32,
    pub min_stored: f32,
    pub max_stored: f32,
    pub reversed: u32,
    pub intrinsics: [f32; 4],
    pub baseline_m: f32,
    pub image_count: u32,
    pub images: [ImageRef; MAX_IMAGES],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Record {
    pub layer_kind: u32,
    pub flags: u32,
    pub generation: u64,
    pub producer_epoch: u64,
    pub colour: ColourGroup,
    pub geometry: GeometryGroup,
}

struct W<'a> {
    b: &'a mut [u8],
    at: usize,
}
impl W<'_> {
    fn u32(&mut self, v: u32) {
        self.b[self.at..self.at + 4].copy_from_slice(&v.to_le_bytes());
        self.at += 4;
    }
    fn u64(&mut self, v: u64) {
        self.b[self.at..self.at + 8].copy_from_slice(&v.to_le_bytes());
        self.at += 8;
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits())
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x)
        }
    }
    fn images(&mut self, count: u32, v: &[ImageRef; MAX_IMAGES]) {
        self.u32(count.min(MAX_IMAGES as u32));
        for i in v {
            self.u32(i.slot_index);
            self.u64(i.acquire_point);
            self.u64(i.release_point);
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}
impl R<'_> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.b[self.at..self.at + 8].try_into().unwrap());
        self.at += 8;
        v
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    fn f32s<const N: usize>(&mut self) -> [f32; N] {
        let mut a = [0f32; N];
        for x in a.iter_mut() {
            *x = self.f32();
        }
        a
    }
    fn images(&mut self) -> (u32, [ImageRef; MAX_IMAGES]) {
        let count = self.u32().min(MAX_IMAGES as u32);
        let mut a = [ImageRef::default(); MAX_IMAGES];
        for i in a.iter_mut() {
            i.slot_index = self.u32();
            i.acquire_point = self.u64();
            i.release_point = self.u64();
        }
        (count, a)
    }
}

/// Bytes actually used by the layout (the rest of a slot is zero).
pub const RECORD_USED: usize = 8 + 8 + 8 // header
    + (8 + 28 + 4 + 4 + 4 + 4 + 4 + 4 + MAX_IMAGES * 20) // colour
    + (8 + 28 + 4 + 64 + 4 + 8 + 20 + 16 + 4 + 4 + MAX_IMAGES * 20); // geometry

impl Record {
    pub fn encode(&self, out: &mut [u8; RECORD_SIZE]) {
        out.fill(0);
        let mut w = W { b: out, at: 0 };
        w.u32(self.layer_kind);
        w.u32(self.flags);
        w.u64(self.generation);
        w.u64(self.producer_epoch);
        let c = &self.colour;
        w.u64(c.t_colour_ns);
        w.f32s(&c.pose_colour);
        w.u32(c.calibration_ver);
        w.u32(c.colour_space);
        w.u32(c.exposure_us);
        w.f32(c.gain);
        w.u32(c.distortion_ref);
        w.images(c.image_count, &c.images);
        let g = &self.geometry;
        w.u64(g.t_geometry_ns);
        w.f32s(&g.pose_geometry);
        w.u32(g.calibration_ver);
        w.f32s(&g.t_geom_to_colour);
        w.u32(g.depth_fourcc);
        w.u64(g.depth_params);
        w.f32(g.near_m);
        w.f32(g.far_m);
        w.f32(g.min_stored);
        w.f32(g.max_stored);
        w.u32(g.reversed);
        w.f32s(&g.intrinsics);
        w.f32(g.baseline_m);
        w.images(g.image_count, &g.images);
        debug_assert_eq!(w.at, RECORD_USED);
    }

    pub fn decode(b: &[u8]) -> Option<Record> {
        if b.len() < RECORD_USED {
            return None;
        }
        let mut r = R { b, at: 0 };
        let mut rec = Record { layer_kind: r.u32(), flags: r.u32(), generation: r.u64(), producer_epoch: r.u64(), ..Default::default() };
        let c = &mut rec.colour;
        c.t_colour_ns = r.u64();
        c.pose_colour = r.f32s();
        c.calibration_ver = r.u32();
        c.colour_space = r.u32();
        c.exposure_us = r.u32();
        c.gain = r.f32();
        c.distortion_ref = r.u32();
        (c.image_count, c.images) = r.images();
        let g = &mut rec.geometry;
        g.t_geometry_ns = r.u64();
        g.pose_geometry = r.f32s();
        g.calibration_ver = r.u32();
        g.t_geom_to_colour = r.f32s();
        g.depth_fourcc = r.u32();
        g.depth_params = r.u64();
        g.near_m = r.f32();
        g.far_m = r.f32();
        g.min_stored = r.f32();
        g.max_stored = r.f32();
        g.reversed = r.u32();
        g.intrinsics = r.f32s();
        g.baseline_m = r.f32();
        (g.image_count, g.images) = r.images();
        Some(rec)
    }

    /// Every image the generation references (colour group, then geometry group).
    pub fn image_refs(&self) -> impl Iterator<Item = ImageRef> + '_ {
        self.colour.images[..self.colour.image_count as usize].iter().copied().chain(self.geometry.images[..self.geometry.image_count as usize].iter().copied())
    }
}

/// The stamp the fake producer writes at the start of every image it publishes, and the test
/// consumer reads back (§8: torn reads, calibration pairing, overwrite-while-in-use are all
/// checked against it). udmabuf makes the images CPU-visible on both sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub producer_epoch: u64,
    pub generation: u64,
    pub calibration_ver: u32,
    pub slot_index: u32,
}
pub const STAMP_SIZE: usize = 24;

impl Stamp {
    pub fn write(&self, image: &mut [u8]) {
        let mut w = W { b: image, at: 0 };
        w.u64(self.producer_epoch);
        w.u64(self.generation);
        w.u32(self.calibration_ver);
        w.u32(self.slot_index);
    }
    pub fn read(image: &[u8]) -> Stamp {
        let mut r = R { b: image, at: 0 };
        Stamp { producer_epoch: r.u64(), generation: r.u64(), calibration_ver: r.u32(), slot_index: r.u32() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_fits_a_slot_and_round_trips() {
        assert!(RECORD_USED <= RECORD_SIZE, "{RECORD_USED}");
        let mut rec = Record { layer_kind: 1, flags: FLAG_COMPLETE, generation: 42, producer_epoch: 7, ..Default::default() };
        rec.colour.image_count = 2;
        rec.colour.images[1] = ImageRef { slot_index: 5, acquire_point: 42, release_point: 42 };
        rec.colour.pose_colour = [0.0, 1.0, 2.0, 0.0, 0.0, 0.0, 1.0];
        rec.geometry.image_count = 6;
        rec.geometry.images[5] = ImageRef { slot_index: 11, acquire_point: 42, release_point: 42 };
        rec.geometry.t_geom_to_colour[15] = 1.0;
        rec.geometry.near_m = 0.1;
        rec.geometry.far_m = 10.0;
        let mut buf = [0u8; RECORD_SIZE];
        rec.encode(&mut buf);
        assert_eq!(Record::decode(&buf), Some(rec));
        assert_eq!(rec.image_refs().count(), 8);
    }

    #[test]
    fn stamp_round_trips() {
        let s = Stamp { producer_epoch: 3, generation: 9, calibration_ver: 2, slot_index: 4 };
        let mut img = [0u8; 64];
        s.write(&mut img);
        assert_eq!(Stamp::read(&img), s);
    }

    #[test]
    fn short_buffer_is_refused() {
        assert!(Record::decode(&[0u8; 16]).is_none());
    }
}
