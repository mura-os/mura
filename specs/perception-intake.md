# specs/perception-intake: the perception→compositor layer contract

**Status:** draft normative spec (specification workstream, wave 2).
**Design sources:** [ADR 0008](../docs/architecture/adr/0008-perception-services-placement.md)
(recast form: ownership decided, execution bound to exactly one of two admissible boundaries),
[perception-passthrough-hands.md](../docs/architecture/perception-passthrough-hands.md)
(the DepthFrame/matte contracts and shared invariants),
[perception-design-backlog.md](../docs/architecture/perception-design-backlog.md) #5/#8 (which
this spec discharges), release/sync semantics per
[research/32 §2](../docs/research/32-toplevel-export-prior-art.md). This is the boundary the zxr
compositor consumes perception through; the services' internals are out of scope.
**Grounding:** buffer identity uses DRM FOURCC + modifier (the linux-dmabuf vocabulary);
timestamps use the Monado monotonic clock domain (`CLOCK_MONOTONIC` unless the device layer
declares otherwise); no XDG sense applies.
**Budget impact** (inv. 9): producers publish at camera/service rate on the perception plane's
existing budget lines; the consumer's per-frame cost is one atomic pointer/fence read per layer
(the latest-complete rule); zero added frame-path blocking by construction (§4).

## 1. Scope and parties

Two producer services (passthrough/environment; hand-cutout/top layer — ADR 0008), one consumer
(zxr's composition intake). The same contract serves both admissible execution placements:

- **in-process sink** (an `xrt_frame` sink inside Monado): the "wire" is a C ABI struct + fence
  handles; §2–§5 field and ordering semantics apply unchanged.
- **adjacent process**: the wire is a SOCK_SEQPACKET control channel carrying the §2 packet as
  flat structs with dmabuf/syncobj fds via SCM_RIGHTS.

A producer declares its placement at registration; mixing semantics is non-conformant (the
ADR 0008 recast's "never both ambiguously").

## 2. The layer packet

One packet describes one publishable unit ("layer generation"). All integers little-endian in
the adjacent-process encoding; all fields mandatory unless marked.

```text
header:
  layer_kind        u32   environment | hand_top            (enum, §2.1)
  generation        u64   monotonically increasing per producer
  t_capture_ns      u64   mid-exposure timestamp, Monado clock (never publish time)
  calibration_ver   u32   bumps atomically on IPD/thermal recalibration; consumer drops
                          cross-version pairings whole
  flags             u32   bitfield: complete | degraded | fabricated_confidence
per view (view_count × ):
  view_id           u32   stable id matching the runtime's view enumeration
  image[]:                one entry per plane_kind present (§2.1 table)
    plane_kind      u32   color | alpha_premul | depth | confidence | guide_luma
    fourcc          u32   DRM FOURCC (FLOAT/FIXED_16(frac)/HILBERT8(order) depth encodings
                          are declared via fourcc+params per research/14 §5)
    modifier        u64
    width, height   u32
    n_planes        u32   dmabuf planes; fds attached out-of-band (SCM_RIGHTS / ABI handles)
    offset,stride[] u32   per dmabuf plane
    params          u64   encoding parameter word (frac bits / Hilbert order / unused = 0)
  depth_range:            present when plane_kind depth present
    near_m, far_m   f32
    reversed        u32
  pose:                   T_world←view at t_capture (the producer's own pose query — in-process
                          by construction; the consumer never re-queries for these pixels)
    position        f32[3]
    orientation     f32[4] unit quaternion x y z w
  intrinsics        f32[4] fx fy cx cy (rectified)
sync (per view, per image):
  acquire_syncobj   fd + u64 point   must signal before the consumer samples
  release_syncobj   fd + u64 point   the consumer signals at GPU-complete; per-image, may
                                     complete out of generation order (never one shared
                                     monotonic timeline across reusable images — research/32 §2)
```

### 2.1 Layer kinds and required planes

| layer_kind | Required planes | Optional |
|---|---|---|
| environment | color, depth, confidence | guide_luma |
| hand_top | alpha_premul (αF), alpha (in FOURCC channel layout), depth | confidence |

Confidence is **always present** for environment (measured / temporally-propagated / completed /
hole classes per the perception doc); a backend unable to produce it must set
`fabricated_confidence` and publish a fabricated plane — absent confidence is non-conformant.

## 3. Publication: the latest-complete register

Producers publish into a **per-layer-kind triple-buffered register** (three generations: one
being written, one latest-complete, one possibly in consumer use):

- A generation becomes *visible* only when fully written and its packet's `complete` flag set —
  partial generations are never visible (stereo atomicity is packet-level: both views or
  nothing).
- The consumer, once per composition pass, atomically takes a reference to the **latest complete
  generation whose acquire points have signalled** ("latest-signalled snapshot") — it must not
  wait on an unsignalled acquire; if none newer is signalled, it reuses its current reference.
- Consumer references pin at most one generation per layer kind; producers may therefore need at
  most three buffers per image slot (write / latest / pinned).

## 4. Ordering and never-block rules (normative)

1. The consumer never blocks on a producer: no waits on acquire points, no reads of
   partially-written generations, no IPC round-trips on the composition path.
2. Producers never block on the consumer: publication overwrites the oldest non-pinned
   generation; a stalled consumer pins at most one.
3. Release points are signalled at consumer GPU-complete per image; a producer must not reuse an
   image before its release point signals **or** the consumer's pin moves on and the register
   slot is reclaimed (whichever the placement's memory model requires — stated per placement in
   §6).
4. Staleness handling is the consumer's (warp from the packet pose to display time, drop if
   older than the layer policy's max age); producers never re-publish identical generations to
   look fresh.

## 5. Failure semantics

- **Producer death** (process exit / sink deregistration): the register is torn down; the
  consumer drops its pinned reference at the next composition pass and composes without the
  layer (policy decides passthrough-missing behavior, not this spec). Outstanding release
  points are considered abandoned; consumers must not signal fds after teardown notice.
- **Timeline reset / device loss**: producer bumps `calibration_ver`'s high bit as an epoch
  marker and re-creates its register; consumers treat unknown epochs as producer death +
  re-registration.
- **Degraded mode**: `degraded` flag marks reduced-quality generations (e.g. mono fallback);
  consumers compose them; policy may badge.

## 6. Placement bindings

- **In-process sink**: the register is a shared struct owned by the producer; visibility uses a
  seqlock (generation counter, acquire/release atomics); fence handles are `xrt_fence`-class
  objects; teardown is sink deregistration. Crash containment: a crashing in-process producer is
  a Monado crash — accepted only for producers meeting the ADR 0008 licensing/stability bar (the
  GPL cutout net is always adjacent).
- **Adjacent process**: registration + packets over SOCK_SEQPACKET (one datagram per packet,
  64 KiB cap, fds via SCM_RIGHTS); the register lives in a memfd-backed ring the producer owns;
  consumer maps read-only. Producer death = socket EOF.

## 7. Conformance checklist

1. Kill the producer mid-generation: consumer's next pass composes without the layer; no torn
   sampling; no fd leaks (release fds closed unsignalled).
2. Stall the consumer 1 s: producer continues at rate, memory bounded to the register size;
   on resume the consumer sees the latest generation, not a queue.
3. Recalibration mid-stream: no frame pairs a new-version pose with old-version pixels.
4. Acquire-not-signalled at pass time: previous generation reused; zero wait measured on the
   composition thread.
5. Out-of-order release across two generations: producer reuse ordering correct (per-image
   points, research/32 §2 rule).

## 8. Open items

The exact `xrt_fence`↔syncobj bridging on the in-process path (Monado internals; with the
implementation); whether hand_top wants a per-pixel depth or a single representative depth in
degraded mode (perception backlog #2's per-client policy interacts); register sizing for
class-B panels (budgets.md bandwidth line).
