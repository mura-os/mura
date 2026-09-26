# 65 — The embedded frame path: where zxr spends on a battery SoC, from comparables and measurement

**Research date:** 2026-09-26. **Question:** the four costs of zxr's per-frame path on a
battery-powered SoC — runtime IPC, GPU, memory, wake-ups — what the comparables and the runtime do
about each, what the dev host can measure, and which decisions follow. Companion to
[research/59](59-xr-compositor-architecture-from-comparables.md) (the program's mechanisms) and
[research/62](62-scene-data-model-from-comparables.md) (the scene). Plan:
`embedded_frame-path_efficiency` (2026-09-26).
**Constraint:** no target hardware. Every number below carries one of three labels —
**measured (host)**: the dev host (AMD Strix Halo, RADV, x86 Unix sockets, Monado simulated HMD
at 60 Hz, `XRT_COMPOSITOR_NULL`), valid on device as *structure* (counts, ratios, A/B deltas),
not as absolute time; **analytic**: computed from resolution × format × pass structure, the
number the hardware would confirm; **hardware-deferred**: cannot be known without the device.
The hardware-deferred set is the first-hardware verification list in
[implementation-path.md §5](../architecture/implementation-path.md).
**Method (AGENTS.md rule 7):** for each cost, the comparables' mechanism *and their reasons*
(`references/<clone>/path:line`; [external] where no clone), the runtime's own behaviour (Monado,
the OpenXR spec), then the measurement. **Budget impact:** a research document; its findings feed
[budgets.md](../architecture/budgets.md) and spec rev 2.

## 0. Instrumentation (Phase 0) and the baseline

Added to zxr's frame journal (`pkgs/zxr/src/journal.rs`, printed on SIGUSR1/exit): a latency
histogram per runtime call (count, mean, max, log2 buckets 16 µs … ≥ 1 ms) for `xrWaitFrame`,
`xrBeginFrame`, `xrLocateViews`, `xrLocateSpaces`, acquire/wait/release per swapchain,
`xrEndFrame`, `xrPollEvent`; the per-frame call census; the analytic attachment traffic of our
pass; frame callbacks split visible/occluded by a frustum test. Harness: strace census;
per-thread wake-ups from `/proc/<pid>/task/*/schedstat`; CPU ms/s from `/proc/<pid>/stat`;
isolated `XDG_RUNTIME_DIR` so a concurrent dev-session is untouched.

### 0.1 Baseline — measured (host), release build, 60 Hz simulated HMD

| | idle (no client) | 1 foot | 3 foot |
|---|---|---|---|
| runtime calls per frame | **11.16** | 11.16 | 11.16 |
| of which block the state loop | 7 (begin, locateViews, acquire ×2, release ×2, end) | 7 | 7 |
| loop time inside runtime calls, per frame | 269 µs | 273 µs | 292 µs |
| `xrBeginFrame` / `xrLocateViews` / acquire / release / `xrEndFrame` mean | 23 / 91 / 87 / 44 / 90 µs (debug, under strace) | | |
| `xrWaitSwapchainImage` | **0 µs — client-local**, not IPC | | |
| `xrWaitFrame` (wait thread, blocking = the runtime's throttle) | 16.5 ms mean = one 60 Hz period | | |
| zxr wake-ups/s: state loop / wait thread | **408 / 146** (≈ 6.8 / 2.4 per frame) | 410 / 144 | 416 / 149 |
| zxr CPU | 7 ms/s (0.7 %) | 10 ms/s | 10 ms/s |
| monado-service wake-ups/s (busiest thread) / CPU | 307 / 4 ms/s | 311 / 3 ms/s | 309 / 4 ms/s |
| GPU per frame (ours, timestamps) | 37 µs (clear only) | 54 µs | 59 µs |
| wake → `xrEndFrame` | 431 µs | 519 µs | 642 µs |
| passes per frame / attachment stores per frame (analytic) | 2 / 7.2 MB (2 × 896×1007 × 4 B) | | |
| frame callbacks sent to occluded planes | 0 (all three planes in view at the fan positions) | | |

Syscall census (strace -c, 600 frames, debug build, includes the foot child): `futex` 4217,
`ioctl` 5215 (~8.7/frame: DRM submits, fence waits), `sendmsg` 9061 / `recvmsg` 10273 (Monado
IPC + Wayland), `epoll_pwait` 1263 (~2/frame), `clock_nanosleep` 600 (**1/frame — inside the
client-side `xrWaitFrame`**, Monado's app pacer sleeps in the client), `timerfd_settime` 652.

**Reading of the baseline.** The state loop wakes ~7 times per frame and 6 of them are the
blocking runtime calls — every IPC round trip is a sleep/wake pair. That is the shape the
embedded numbers will follow: **wake-ups per frame ≈ 1 + runtime calls on the loop**, and CPU
time per frame is dominated by those context switches, not by the scene or the draw list (7 ms/s
total at 60 Hz idle, of which the calls are ~270 µs × 60 = 16 ms/s of *wall* time blocked, most
of it not on-CPU). GPU time is noise on this host; its structure (2 passes, 7.2 MB of stores)
is what transfers.

*(Sections 1–4 follow the plan's phases; filled as each read and measurement completes.)*
