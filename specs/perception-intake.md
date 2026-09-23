# specs/perception-intake: the perception→compositor layer contract

**Status:** draft rev 2 (specification workstream; rev 1 findings from the Monado-persona review
absorbed — dual-rate packet restored, GPU-safe pool model, registration-based fd transfer).
**Design sources:** [ADR 0008](../docs/architecture/adr/0008-perception-services-placement.md)
(recast), [perception-passthrough-hands.md](../docs/architecture/perception-passthrough-hands.md)
(the two-clock DepthFrame contract this spec now carries in full),
[perception-design-backlog.md](../docs/architecture/perception-design-backlog.md) #5/#8
(dispositions in §9), sync semantics per
[research/32 §2](../docs/research/32-toplevel-export-prior-art.md).
**Grounding:** DRM FOURCC + modifier for images; Monado monotonic clock for all timestamps; no
XDG sense applies.
**Budget impact** (inv. 9): producers publish at camera/service rate on existing perception
lines; the consumer performs one bounded, non-retrying register read per composition pass; pool
sizes are negotiated and fixed (§4). No frame-path blocking by construction (§5, conformance 4).

## 1. Scope, parties, and the settled domain question

Producers: the passthrough/environment service and the hand-cutout service (ADR 0008). Consumer:
zxr's composition intake. **The source-vs-final fork (backlog #5) is settled here: producers
export source-domain artifacts; the compositor owns the single display-time warp.** The
environment producer publishes camera-domain colour and geometry with their own clocks and poses;
zxr warps each to display time in its composition pass (the "compositor owns only the
display-rate warp" rule of the ADR 0008 recast). Consequently the compositor submits its own
depth policy to the OpenXR runtime and runtime-side depth reprojection of these layers is
disabled — double-reprojection cannot occur.

The two admissible execution placements (ADR 0008) share every rule below:

- **in-process sink**: the wire is a C ABI over the §4 register; handles are `xrt_fence`-class.
- **adjacent process**: a SOCK_SEQPACKET control channel (§7) plus a producer-owned memfd
  register mapped read-only by the consumer; dmabufs and syncobj timelines are transferred
  **once, at registration**, never per generation.

## 2. The generation record (dual-rate, per the DepthFrame contract)

One record describes one publishable generation of one layer kind. The environment layer is
explicitly **dual-rate**: colour and geometry age independently and each carries its own
timestamp, pose, and calibration reference.

```text
header:
  layer_kind          u32   environment | hand_top
  generation          u64   monotonic per producer epoch
  producer_epoch      u64   from registration; bumps on device loss / producer restart (§6)
  flags               u32   complete | degraded | fabricated_confidence
colour group (environment: required; hand_top: the αF/α images):
  t_colour_ns         u64   mid-exposure, Monado clock (never publish time)
  pose_colour         f32[7]  T_world←camera at t_colour (pos xyz + quat xyzw)
  calibration_ver_c   u32
  colour_space        u32   enum (sRGB-nonlinear | BT.601 | BT.709 ...)
  exposure_us, gain   u32, f32
  distortion_ref      u32   index into registered distortion maps (§3)
  images[]                  slot indices into the registered image table (§3)
geometry group (environment: required; hand_top: the hand-depth image):
  t_geometry_ns       u64   may differ from t_colour (dual rate)
  pose_geometry       f32[7] T_world←rig at t_geometry
  calibration_ver_g   u32
  T_geom_to_colour    f32[16] alignment transform, column-major
  depth encoding:     fourcc + params u64 (FLOAT | FIXED_16(frac) | HILBERT8(order))
  depth_range:        near_m f32, far_m f32, min_stored f32, max_stored f32, reversed u32
                      — the canonical mapping of zxr_frame_slot_v2.set_depth_range: stored
                      s∈[min,max] → window depth → reciprocal-linear distance in [near,far]
  intrinsics          f32[4] fx fy cx cy (rectified) + baseline_m f32
  images[]                  depth, confidence (REQUIRED; classes measured/propagated/
                            completed/hole), guide_luma (REQUIRED for environment, full-res)
per image reference:
  slot_index          u32   into the registered image table
  acquire_point       u64   on the image's registered acquire timeline
  release_point       u64   on the image's registered release timeline (per-image timelines;
                            never one shared monotonic release timeline — research/32 §2)
```

Producer device identity, image formats/modifiers/strides, distortion maps, and timelines are
**registration-time** data (§3), not per-generation fields.

## 3. Registration

At attach, the producer registers once: its identity (name, device id, `producer_epoch`), the
image table (every dmabuf it will ever publish: fd, fourcc, modifier, planes/offsets/strides,
dimensions, usage class), per-image acquire/release syncobj timelines, distortion maps, and the
negotiated **`max_in_flight`** (§4). Adding images requires a re-registration message; the
consumer acks before first use. This bounds fd transfer to registration and makes generation
records small and fixed-layout.

## 4. Publication: the GPU-safe latest-signalled register

- The register holds `pool = 2 + max_in_flight` generation slots per layer kind (one being
  written, one latest-complete, `max_in_flight` potentially referenced by consumer GPU work).
  `max_in_flight` is negotiated at registration (consumer declares; minimum 1, typical 2).
- Visibility: a generation becomes visible only when fully written and flagged complete
  (packet-level stereo atomicity — both views' groups or nothing). Publication uses fixed
  pre-registered slots and a single atomic latest-index store with release ordering; **the
  consumer performs one acquire-ordered read of the latest index and its slot header per pass —
  no retry loop, no lock** (a torn write is impossible by construction: slots are only rewritten
  after reclamation, §4.3).
- Selection: the consumer takes the newest visible generation **whose acquire points have
  signalled**; otherwise it keeps its current generation. It never waits.
- **Reclamation (the GPU-safety rule): a slot is reusable only when every release point of every
  submitted consumer use of its images has signalled.** The consumer moving its snapshot forward
  is *never* a release condition. If no slot is reclaimable, the producer drops the new
  generation (a counted overrun, §7) rather than overwriting — producers never block, and
  memory is bounded by the pool.

## 5. Never-block rules (normative)

1. The consumer never blocks on a producer: one non-retrying register read per pass; no fence
   waits on the composition thread; unsignalled acquire ⇒ reuse current.
2. The producer never blocks on the consumer: overrun-drop per §4; registration acks are
   asynchronous.
3. Staleness is the consumer's: warp each group from its own pose/time to display time; drop
   layers older than the layer policy's max age. Producers never republish identical
   generations.

## 6. Failure semantics

- **Producer death** (socket EOF / sink deregistration): the consumer stops selecting new
  generations immediately, composes without the layer from its next pass, **but retires nothing
  early**: registered images and timelines are retained until every submitted GPU use completes
  (or device loss makes completion impossible), then unmapped and closed. Abandoned release
  points are closed unsignalled only after that retirement — the same rule as
  `zxr_frame_slot_v2.destroy`.
- **Device loss / producer restart**: a new registration with a higher `producer_epoch`
  supersedes the old identity; the old epoch's teardown follows the death rule. Epochs are
  independent of `calibration_ver_*`, which count calibration changes only.
- **Degraded** generations (flag) are composed; policy may badge.

## 7. Adjacent-process encoding

Versioned, fixed-layout little-endian records over SOCK_SEQPACKET: `magic ("SPIN") | version u32
| byte_length u32 | type u32 | body`. Message types: `REGISTER` (identity + tables; fds via
SCM_RIGHTS, at most 16 per datagram, continued across `REGISTER_MORE` datagrams with explicit
`fd_base` indices), `REGISTER_ACK`, `GENERATION` (the §2 record — fixed maxima: ≤ 4 views, ≤ 6
images per group, so ≤ 1 KiB, no fds), `OVERRUN` (count report), `GOODBYE`. Unknown types with
`version` ≤ negotiated are ignored; higher versions are a registration failure. The memfd ring
carries the §2 records at the slot granularity of §4; datagrams are notifications and control
only, so SEQPACKET buffer limits never carry image data.

## 8. Conformance checklist

1. Kill the producer mid-generation: next pass composes without the layer; no torn read; images
   unmapped only after GPU completion (validated by fence introspection); no fd leaks.
2. Stall the consumer 1 s: producer drops with OVERRUN counts, memory bounded to the pool; on
   resume the consumer reads the latest generation, not a queue.
3. Recalibration: no composed frame pairs pixels and pose across `calibration_ver` values within
   a group; dual-rate pairing across groups uses `T_geom_to_colour` of the geometry group only.
4. **Structural never-block test**: with the acquire point held unsignalled and the producer
   writing continuously, the composition thread is traced (syscall + fence-wait deny-list) for N
   frames: zero blocking syscalls/fence waits, previous-generation reuse observed via the
   fallback counter — not inferred from elapsed time.
5. Out-of-order release across two in-flight generations: reclamation respects per-image
   release points (never the shared-timeline shortcut).

## 9. Backlog dispositions

- **#5**: field-by-field — dual timestamps/poses/calibrations (§2 colour/geometry groups),
  alignment transform (§2), colour space/exposure/gain (§2), distortion (registered, §3),
  producer device id (§3), reuse lifetime (§4 reclamation rule), stereo atomicity (§4
  visibility), source-domain decision (§1). Discharged.
- **#8**: transport = registration + fixed-layout records + memfd register + per-image syncobj
  timelines with acquire/release points, latest-signalled selection (§4), producer-death and
  epoch reset (§6), backpressure = bounded pool + overrun-drop (§4/§7). Discharged.

## 10. Open items

The `xrt_fence`↔syncobj bridge specifics on the in-process path (Monado internals); hand_top
degraded-mode depth representation (perception backlog #2 interaction); per-device pool sizing
for class-B panels (budgets.md bandwidth line).
