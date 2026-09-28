//! `wl_shm` `ARGB8888` as a Slint software-renderer target: little-endian 0xAARRGGBB, i.e. bytes
//! B, G, R, A in memory — the one format every compositor must offer (`wl_shm` spec) and the
//! one zxr advertises. Slint's own `PremultipliedRgbaColor` is byte-order R, G, B, A (`ABGR8888`
//! on the wire), which zxr does not advertise; this type is the same premultiplied blend with the
//! channels swapped (`references/slint/internal/renderers/software/draw_functions.rs:874-885`).

use slint::platform::software_renderer::{PremultipliedRgbaColor, TargetPixel};

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Bgra8 {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl TargetPixel for Bgra8 {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let a = (u8::MAX - color.alpha) as u16;
        self.r = (self.r as u16 * a / 255) as u8 + color.red;
        self.g = (self.g as u16 * a / 255) as u8 + color.green;
        self.b = (self.b as u16 * a / 255) as u8 + color.blue;
        self.a = (self.a as u16 + color.alpha as u16 - (self.a as u16 * color.alpha as u16) / 255) as u8;
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Bgra8 { b, g, r, a: 0xff }
    }

    fn background() -> Self {
        Bgra8 { b: 0, g: 0, r: 0, a: 0 }
    }
}

/// View a `wl_shm` mapping as pixels (the pool hands out `&mut [u8]`).
pub fn as_pixels(bytes: &mut [u8]) -> &mut [Bgra8] {
    let n = bytes.len() / 4;
    // SAFETY: `Bgra8` is `repr(C)` of four `u8`s with alignment 1; the slice is reinterpreted
    // over whole pixels only.
    unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut Bgra8, n) }
}
