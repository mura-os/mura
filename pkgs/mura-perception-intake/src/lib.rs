//! perception_intake — the perception→compositor intake (specs/perception-intake.md), the
//! adjacent-process placement (§1): a SOCK_SEQPACKET control channel (§7), a producer-owned memfd
//! register the consumer maps read-only (§4), dmabufs and per-image syncobj timelines transferred
//! once at registration (§3), fixed-layout generation records (§2).
//!
//! This library is the protocol; `intake-fake-producer` and `intake-test-consumer` are its §8
//! conformance harness and never ship in an image. The real producers (the passthrough and
//! hand-cutout services) and zxr's intake are meant to link this crate.
//!
//! libc only: the wire is fixed-layout little-endian, the kernel is reached through raw ioctls
//! (DRM syncobj, udmabuf) — no libdrm, no serde.

pub mod kernel;
pub mod record;
pub mod register;
pub mod seqpacket;
pub mod usepage;
pub mod wire;

/// The spec's two layer kinds (§2 `layer_kind`). Fixed: a new kind is a spec revision.
pub const LAYER_ENVIRONMENT: u32 = 1;
pub const LAYER_HAND_TOP: u32 = 2;

pub fn layer_name(kind: u32) -> &'static str {
    match kind {
        LAYER_ENVIRONMENT => "environment",
        LAYER_HAND_TOP => "hand_top",
        _ => "unknown",
    }
}

pub fn parse_layer(name: &str) -> Option<u32> {
    match name {
        "environment" => Some(LAYER_ENVIRONMENT),
        "hand_top" => Some(LAYER_HAND_TOP),
        _ => None,
    }
}
