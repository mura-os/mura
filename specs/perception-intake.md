# specs/perception-intake: the perception→compositor layer contract

**Status:** draft rev 3 — rev 2 plus the §8 harness's finding (2026-09-25,
`pkgs/mura-perception-intake`, `nix build .#vm-test-perception-intake`): reclamation needs the
consumer's *use declaration* beside the fences (§4.4, §7 REGISTER_ACK); §8 restated as observed.
**Rev 3 is the harness's proposal, not yet ruled** — the change is marked ⚠ below.
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
- ⚠ **4.4 The use declaration (rev 3).** The fences cannot say whether a use was ever
  *submitted*: a generation superseded before the consumer looked at it has no release point
  that will ever signal, and a producer relying on fences alone cannot tell it from one in
  flight — the pool wedges after `pool` skipped generations (the harness found this under a
  slow consumer). Every comparable gives the producer that fact from the consumer: the
  explicit-sync release point is *"signalled by the compositor when it has finished its usage"*
  and *"compositors may release buffers without ever reading from them"* — the compositor owes it
  for every committed buffer, used or not (`references/wayland-protocols/staging/linux-drm-syncobj/linux-drm-syncobj-v1.xml:210-222`;
  the same file's warning against one shared release timeline, `:226-231`, is research/32 §2's);
  Vulkan WSI's presentation engine hands images back to the application through
  `vkAcquireNextImage` ([external], the Vulkan specification, WSI swapchain). The reason
  transfers as is: only the consumer knows it never used a generation. Wayland's compositor can
  release each buffer because it *sees every commit*; ours reads only the latest (§5), so the
  declaration is a table it keeps current rather than an event per generation. Here the consumer
  owns a **use page** — one page-sized memfd sent once in
  `REGISTER_ACK` (§7), mapped read-only by the producer — with `pending[]` (generations with a
  submitted, not-yet-completed use; withdrawn when the fences signal) and `intent` (the
  generation being taken this instant). Reclaim rule: a non-latest slot is reusable when its
  generation is **not in `pending` and not `intent`** (never used), or is in `pending` **and**
  every release point signalled. The two sides agree on a slot with two flags, each stored
  before the other's is read (SeqCst): the producer stores `reclaiming = G` in the register
  header, then reads the page; the consumer stores `intent = G`, then reads `reclaiming` and
  re-reads the slot's generation. At least one sees the other and backs off — the consumer keeps
  its current generation, the producer tries another slot or drops. No lock, no wait, no retry
  loop; the consumer's pass gains one store and two loads on a selection attempt. Cost of the
  omission had the harness not caught it: silent pool exhaustion whenever the compositor
  falls behind the producer. If `pending[]` is full, the consumer clears `intent` and refuses
  the selection **before** submitting GPU work; an undeclared live use is forbidden. The
  comparables establish the consumer's release/declaration **obligation**, not this mechanism:
  the use page and two-flag exchange are a Mura-specific proposal chosen to preserve the
  never-block rule without a `TAKEN` round trip. Decider: the owner (rule 8).

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
SCM_RIGHTS, at most 16 per datagram — the register memfd first, then dmabuf/acquire/release per
image entry — continued across `REGISTER_MORE` datagrams with explicit `fd_base` indices; the
consumer acks once `image_count` entries have arrived), `REGISTER_ACK` (status + the consumer's
`max_in_flight`; ⚠ rev 3: on success one fd, the consumer's use page, §4.4), `GENERATION` (the §2
record — ≤ 6 images per group, so ≤ 1 KiB, no fds; a notification — the register is
authoritative), `OVERRUN` (count report), `GOODBYE`. Unknown types with
`version` ≤ negotiated are ignored; higher versions are a registration failure. The memfd ring
carries the §2 records at the slot granularity of §4; datagrams are notifications and control
only, so SEQPACKET buffer limits never carry image data.

## 8. Conformance checklist

Verified 2026-09-25 by `nix build .#vm-test-perception-intake` (`tests/vm/perception-intake.nix`;
`intake-fake-producer` + `intake-test-consumer`, udmabuf images stamped with
`{epoch, generation, calibration_ver, slot}` so pixels can be checked against records, syncobj
timelines on the VM's virtio-gpu render node). Each item states what was observed.

1. Kill the producer mid-generation: next pass composes without the layer; no torn read; images
   unmapped only after GPU completion (validated by fence introspection); no fd leaks.
   **Verified** — SIGKILL at 60 Hz: `layer_absent` from the next pass, 0 stamp mismatches; with
   the fake GPU held 500 ms past the death, the fd count stayed at the registered level until
   retirement 509 ms later and returned to baseline (+ the socket) after it.
2. Stall the consumer 1 s: producer drops with OVERRUN counts, memory bounded to the pool; on
   resume the consumer reads the latest generation, not a queue.
   **⚠ Not verified as the rev-2 criterion under the unruled rev-3 proposal.** With §4.4, a
   conforming stalled consumer instead allowed skipped never-used generations to be reclaimed
   (180 published, 0 dropped, ≥170 reclaimed, RSS flat), then jumped ≈60 generations in one
   selection. The OVERRUN mechanism itself was separately verified with a deliberately
   non-conforming consumer that exceeded its declared in-flight limit (`--exceed-in-flight 6`,
   300 ms fake GPU): matching drop counts, no overwrite, publication resumed after releases.
   Ruling §4.4 decides which expectation becomes normative.
3. Recalibration: no composed frame pairs pixels and pose across `calibration_ver` values within
   a group; dual-rate pairing across groups uses `T_geom_to_colour` of the geometry group only.
   **Verified** — one `calibration_ver` change mid-run, 120 selections, 0 group mismatches.
4. **Structural never-block test**: with the acquire point held unsignalled and the producer
   writing continuously, the composition thread is traced (syscall + fence-wait deny-list) for N
   frames: zero blocking syscalls/fence waits, previous-generation reuse observed via the
   fallback counter — not inferred from elapsed time.
   **Verified** — 300 passes under `strace -f` between the consumer's `PASSES_BEGIN`/`PASSES_END`
   markers; the window contained only `clock_gettime`, `ioctl` (`SYNCOBJ_QUERY`,
   `SYNCOBJ_TIMELINE_SIGNAL`) and `recvmsg(MSG_DONTWAIT)`; deny-list hits 0 (`nanosleep`,
   `futex`, poll family, `read`, `SYNCOBJ_*WAIT`, …); `fallback_reused_current` > 100 of 300,
   `nonzero_timeout_waits` = 0.
5. Out-of-order release across two in-flight generations: reclamation respects per-image
   release points (never the shared-timeline shortcut).
   **Verified** — every other use released its second image 3× later than the next use's; 25
   out-of-order completions observed, 0 slots rewritten while any image of a use was held.
6. A full use-page pending table refuses the selection before GPU submission; the generation
   remains reclaimable rather than becoming a live undeclared use.
   **Verified** — `max_in_flight=17` filled the 16-entry table; refusals were counted, with zero
   stamp mismatches and zero rewrites while in use.

Also verified: REGISTER + 6× REGISTER_MORE carrying 32 images × 3 fds + the memfd; the ack
asynchronous (the producer publishes before it; the consumer selects after it); epoch
supersession (a restart with `producer_epoch + 1` on the same connection: both tables mapped
during the overlap, the old epoch retired under item 1's rule); `GOODBYE`; an unknown type at our
version ignored; a future version refused (the producer exits 3); the `hand_top` layer kind
(6 images a set). Not verifiable in the VM: real GPU completion (the fake GPU is the consumer
thread signalling the timelines — the semantics, not the latency).

## 9. Backlog dispositions

- **#5**: field-by-field — dual timestamps/poses/calibrations (§2 colour/geometry groups),
  alignment transform (§2), colour space/exposure/gain (§2), distortion (registered, §3),
  producer device id (§3), reuse lifetime (§4 reclamation rule), stereo atomicity (§4
  visibility), source-domain decision (§1). Discharged.
- **#8**: transport = registration + fixed-layout records + memfd register + per-image syncobj
  timelines with acquire/release points, latest-signalled selection (§4), producer-death and
  epoch reset (§6), backpressure = bounded pool + overrun-drop (§4/§7). Discharged.

## 10. Open items

⚠ Rev 3's use declaration (§4.4) — decider: the owner. The `xrt_fence`↔syncobj bridge specifics
on the in-process path (Monado internals; the in-process placement needs the same declaration,
as a field the sink writes); hand_top degraded-mode depth representation (perception backlog #2
interaction); per-device pool sizing for class-B panels (budgets.md bandwidth line); whether the
structural never-block check moves in-thread (a seccomp filter over the pass) when zxr's intake
exists — the tracer is the harness's, the reserved hook is the compositor's.
