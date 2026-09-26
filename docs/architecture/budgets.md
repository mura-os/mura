# Mura architecture: budgets (frame, compute, power, thermal)

**Status:** draft (gap-closure workstream). This document owns the *global* contention model —
one SoC, hard frame deadlines, a battery, and a fanless (or nearly fanless) thermal envelope,
shared by every plane at once. Individual docs own their components' internals; this document
owns the partition between them and **the discipline rule that keeps it owned**.

The posture, stated once: Mura is effectively near-embedded development. Perception,
composition, clients, and services contend for a smartphone-class SoC while a headset demands
desktop-GPU-grade deadlines. Efficiency is not an optimization pass; it is an architectural
invariant ([overview.md](overview.md) invariant 9).

## 1. The standing rule (the discipline mechanism)

Mirroring the donor standing rule in [design-backlog.md](design-backlog.md):

> **Every new ADR and component design doc carries a "Budget impact" statement** — which budget
> lines of §3 it consumes (frame-path time, async compute rate, NPU/HTP occupancy, memory
> bandwidth, idle watts), an estimate or a named measurement gate, and what it displaces.
> "Negligible" is a legal value; *absent* is a review blocker. Registry rows may cite their
> budget line. Milestone acceptance tests (composition M-series, mapping M-series, perception
> tiers) include their budget measurements once hardware exists.

Retroactive coverage: the ADRs already ratified get budget lines in §3–§5 below rather than
amendments; new work starts carrying the statement from now.

## 2. The frame-budget model

Two clock domains, deliberately decoupled (ADR 0008's latest-complete rule):

- **The display path (hard deadline, per refresh):** Monado's compositor + zxr's composition
  pass + late-latch warp must fit the refresh period — **13.9 ms @ 72 Hz, 11.1 ms @ 90 Hz** —
  minus the runtime's own compositor slice. zxr's composition is bounded and window-count-linear
  (sort-last colour+depth resolve + quad rasterization); *nothing else is ever admitted to this
  path* (no ML, no stereo matching, no policy/scripting, no effects beyond capped transforms —
  composition doc §7.4 deadline rule, ADR 0008).
- **Async producers (rate-budgeted, never frame-blocking):** perception services (VIO, mapping,
  passthrough, cutout, hands, eyes), clients (their own render loops against forwarded pacing),
  shell/services (damage-driven). Each has a *rate* and an *occupancy* budget, not a slot in the
  frame; staleness is handled by warp/reuse, not by waiting.

## 3. The partition (per plane; device classes parameterized by contract facts)

Device classes (from `mura.hardware.*`): **A** = XR2 Gen 1 class (Lynx R1, Quest 1: 72 Hz,
~1600p×2), **B** = XR2+ Gen 2 class (Galaxy XR, Play For Dream: 90 Hz, 3552×3840×2 — the panel
that makes everything hard), **C** = SM8650/Steam Frame (Linux-native, 90–120 Hz), **V** = dev
VM/desktop (correctness only, no budget gates).

| Budget line | Owner (plane) | Class-A/C target | Class-B note | Evidence today |
|---|---|---|---|---|
| Display path: Monado compositor + distortion | perception (runtime) | ≤ 2.5 ms/frame | UBWC scanout + hardware pacer proven on-device | galaxyxr fork: steady 90 fps with fused passthrough ([31 §5](../research/31-kwin-vr.md)) |
| Display path: zxr composition pass | authority | ≤ 2 ms/frame @ ~10 windows | multiview mandatory; foveated shading-rate reclaims ~9 GPU pts | KWin VR: 0.3–1 ms main + 1.3–1.9 ms worker @ ~10 windows ([31 §2.8](../research/31-kwin-vr.md)); FSR foveation 77%→68% GPU ([31 §5](../research/31-kwin-vr.md)) |
| Client GPU (all apps together) | app boundary | the remainder after display path + perception GPU; compositor pre-empts (priority queues) | per-app budgets are policy (observer-view budget objects are the sharing precedent) | WiVRn/ALVR encode-share numbers (doc 18) as contention examples |
| VIO (Basalt) | perception | camera-rate, CPU-bound, ≤ 1 core sustained | — | doc 20 characterization |
| Passthrough + cutout | perception | camera-rate; depth backend duty-cycled | classical Vulkan baseline until BSP paths verified (P-1) | perception doc §depth backend; "12 ms" endpoint claims quarantined (doc 16) |
| Mapping/geometry services | perception | duty-cycled, background priority; bounded by M0 gate metrics | — | spatial-mapping §11 |
| NPU/HTP | perception (single scheduler) | **one owner at a time**: Mercury / depth / ET / reloc are *scheduled*, never concurrent-by-accident | ET+foveation adds a continuous tenant | doc 28 open q4 (contention flagged); galaxyxr QNN ET |
| Shell + service planes | shell/service | damage-driven only; **zero steady-state CPU wake-ups when idle**; panels/OSDs never animate uncapped | — | constraint 6 (comfort caps double as power caps) |
| Idle/docked floors | system | ADR 0015 quiescence: soft-idle (Monado alive) and deep-idle (perception stopped) reach the flat-output-only power profile | dock+doff must beat undocked-idle | [ADR 0015](adr/0015-docked-desktop-mode.md) ladder |
| Memory bandwidth | cross-cutting | UBWC/compressed scanout everywhere the BSP allows; no un-tiled full-res copies on the frame path | class-B panel = the bandwidth wall | galaxyxr UBWC evidence; doc 17 §6.4 dmabuf rules |
| Thermal | system (power/thermal daemon — registry row, still missing) | sustained > burst: budgets are set at the *sustained* clock, burst is headroom | — | device-contract sustained-thermal qualification tier |

Numbers marked "target" are provisional partitions, not measurements; they become measured gates
per the standing rule when hardware lands. The table is deliberately coarse — its job is to make
*ownership* undeniable, so no milligram of frame time or watt is unowned.

## 4. Contention rules (normative)

1. **The frame path is a whitelist** (§2); everything else is async and preemptible by it.
2. **One NPU scheduler.** HTP/NPU tenants register with the perception plane's scheduler;
   concurrent tenancy is a scheduling decision with a budget line, never an accident.
3. **Compositor GPU priority above clients** (Monado already runs its compositor high-priority;
   zxr's composition inherits the same class), clients above background perception (mapping,
   reloc), which are duty-cycled.
4. **Idle means idle.** Service-plane daemons are socket-/bus-activated (greetd/systemd
   precedent); constraint 6's motion caps also bound wake-up-causing animation; the appliance
   idle ladder (ADR 0007) and docked quiescence (ADR 0015) are the two enforcement points.
5. **Budget lines move only here.** Re-partitioning is an edit to §3 with the affected owners
   cross-referenced — not a silent renegotiation inside one component's doc.

## 5. Measurements we already hold (the seed evidence)

- KWin VR frame decomposition (Radeon 890M, 5376×1512@60): Vulkan multiview async main 300 µs /
  worker 3.07 ms; ~10-window desktop main ≈0.7–1 ms; Intel UHD 600 floor: usable at 87–98% GPU
  ([31 §2.8](../research/31-kwin-vr.md)).
- Galaxy XR (galaxyxr Monado fork): 90 fps sustained with titan passthrough fused in the
  distortion pass; gaze-driven two-level FSR foveation: GPU busy 77% → 68%
  ([31 §5](../research/31-kwin-vr.md)).
- Depth/hand/ET model costs: per-model characterizations in docs 14/15/28; XR2-class splat
  rendering feasibility in doc 27.
- zxr R0 on the dev host (RADV, simulated HMD, 896×1007 per view; research/61, research/65 —
  **host-measured, not device numbers; the structure transfers, the times do not**): 13 runtime
  RPCs per frame (11 on the state loop), ~20 loop wake-ups per frame; loop time inside runtime
  calls 0.2–0.3 ms/frame; our GPU pass 44–109 µs; 2 passes and 7.2 MB of attachment stores per
  frame (analytic); RSS anon 7.5 MB + binary 2.7 MB + one ICD 4.8 MB. With panels as runtime
  quad layers: 5 RPCs, ~10 wake-ups, 0 GPU for static UI under head motion, Monado's cost flat
  Beside a native OpenXR game (xrgears as Monado's main session; research/67 §2.1, medians of
  three 20 s runs, ±5 ms/s): game alone Monado 32 ms/s GPU; + zxr quiet (0 layers) 37 (fast path
  kept), + 1 quad 37, + 16 quads 52 (≈ 0.02 ms per quad here; Quest publishes 0.1 ms per layer,
  0.6 ms per fullscreen layer on a Quest 2), + a full-view layer 34–45; quiet zxr 8 ms/s CPU +
  5 ms/s in `monado-service`; session recreation to `FOCUSED` 37–41 ms. A client committing
  while zxr is quiet (research/69 §0, buffers released at replacement): a frame-callback-respecting
  client 5–8 ms/s at 1 commit/s; a GPU-bound client ignoring them 36 µs of zxr per commit — ≈ 3.6 ms/s
  per 100 fps, +17 % rate over composed; the trivial-renderer bound 500 ms/s at 29 k commits/s
  (the client pays 915). A composed GPU-bound Mesa EGL client at ≈ 1 k fps costs 450 ms/s and
  75 k wake-ups/s through the roundtrip spin of research/69 §4.2 — the frame path's open number.
  (research/65 §2.3). **Analytic device band:** 11 RPCs × 20–60 µs aarch64 socket RTT =
  0.2–0.7 ms of a 90 Hz frame; at 2 × 1832×1920 the projection path stores 28 MB and Monado
  reads 28 MB per frame, 5 GB/s at 90 Hz; a panel blit costs 2 × its bytes per commit.
- What we lack (open): per-device power rails, sustained-clock tables, camera-pipeline
  bandwidth; the zxr numbers above on a tiler GPU and an aarch64 socket (the first-hardware
  verification list, [implementation-path.md §5.1](implementation-path.md)).

## 6. Cross-references

Consumers and enforcement points: [overview.md](overview.md) invariant 9;
[zxr-shell-v2-composition.md](zxr-shell-v2-composition.md) §7.4 deadline rule + §7.3
constraints 6–9; [ADR 0008](adr/0008-perception-services-placement.md) (never-block rule);
[ADR 0015](adr/0015-docked-desktop-mode.md) (quiescence); perception/mapping backlogs (NPU +
fan-out bounds); [component-registry.md](component-registry.md) (rows may cite lines above).
