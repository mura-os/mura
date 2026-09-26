# 61 — R0 bring-up results: the four gates of specs/zxr-core.md §12, measured

**Date:** 2026-09-26. **What this is:** the proof run of [specs/zxr-core.md](../../specs/zxr-core.md)
rev 1 — `pkgs/zxr` (commit `5f45836` + the gate-driven changes recorded in §6) in the
`dev-session --zxr` slot on the dev workstation: Monado `monado-service` with the simulated HMD
(two views 896×1007, 60 Hz, `SIMULATED_ROTATE` where stated), AMD Radeon 8060S (RADV, Strix
Halo), host Wayland session. No VM, no headset. Every number below is read from the frame
journal (`--journal`, SIGUSR1, `zxr ctl SOCKET journal`) of a run whose log and journal are
quoted; the harness commands are the control socket's (`zxr ctl`), so the runs are repeatable
by hand. The plan named this file `research/60-…`; 60 was taken by the DE-abstractions doc.
**Budget impact:** a research document; the numbers feed the spec's fence (rev 2).

## 1. Summary

| gate | threshold (spec §12) | measured | result |
|---|---|---|---|
| 1 real presentation | < 1 % missed deadlines over 600 frames; projection layer; movable | 0 / 600 missed (0 / 900, 0 / 3464 in later runs); GPU 79 µs mean / 122 max; `move`, `resize`, `focus` over the seat and control socket | **pass** |
| 2 real GPU integration | 0 CPU copies; device-derived feedback table; syncobj acquire + release end to end; retention ≤ 2 frames under a fast client | 4 dmabuf imports for a 4-image swapchain, 0 copies; 16-entry format/modifier table from `vkGetPhysicalDeviceFormatProperties2`; **245 752 syncobj acquires, 0 implicit** from a Vulkan client in MAILBOX mode committing 14.7 k/s; retention max 2, mean 2.0; 0 missed | **pass** |
| 3 churn | resize, positioner popups, focus handoff, `kill -9` mid-frame, destroy with in-flight GPU work: no unresolved waits, no stale textures, ≤ 1-frame stalls | `resize 900 600` → client 900×598; GTK3 menus as `xdg_popup`s (1 → 3 nested) via the seat; focus handoff on kill; vkcube `kill -9` while committing at 14.7 k/s with its dmabufs in flight; foot `kill -9`; GTK client killed with a menu open — `stale_texture_draws = 0`, `fences_outstanding = 2` (the two slots), 0 missed, compositor alive | **pass** |
| 4 Xwayland | one X11 app through the ruled path | xterm through xwayland-satellite mapped as an 884×556 plane; keystrokes synthesised at zxr's seat arrived in xterm (`cat > file` holds the typed line); X11 buffers were dmabufs on the **implicit**-sync path (8 acquires) — both acquire paths exercised in one session | **pass** |
| fence | binary ≤ 40 MB; RSS ≤ 60 MB nested with one client; ≤ 4 threads; 0 copies | binary 4.8 MB (release); RSS 55.5 MB with one client of which **7.5 MB anon** — 44.5 MB is the host's Vulkan loader mapping three ICDs (libLLVM 26.5 MB, llvmpipe, Dozen); 5 threads of which 2 are Mesa's disk-cache workers; 0 copies | **pass on the process, host-inflated on RSS** (§5) |
| fallback trigger | a structural smithay defect | none met; what smithay lacked was cosmetic (§7) | **not triggered** |

## 2. Gate 1 — real presentation

Run `r1`: `dev-session --zxr -- --frames 600 --journal …` (foot spawned by the harness).

```
frames=600  frames_rendered=598  missed_deadlines=0
gpu_us_mean=79  gpu_us_max=122
wake_to_end_us_mean=436  wake_to_end_us_max=3967
shm_uploads=19  frame_callbacks=598  toplevels_mapped=1  retention_frames_max=2
call_wait_frame_us_mean=16614 (the runtime's throttle: 60 Hz)
call_begin_frame_us_mean=12  call_locate_views_us_mean=32  call_acquire_image_us_mean=51
call_release_image_us_mean=21  call_end_frame_us_mean=47
runtime_calls_per_frame_x100=1104  runtime_calls_loop_us_per_frame=236
```

The two unrendered frames are the first two ticks (session `SYNCHRONIZED` before `FOCUSED`,
`shouldRender = false`). "Missed" is wake→`xrEndFrame` exceeding the predicted period; the worst
frame (4.0 ms) is the first shm upload (a 696×494 staging copy on the queue with a fence wait —
the one CPU-side copy the design allows, counted separately). Foot committed 19 times in 10 s:
frame callbacks throttle a FIFO client to what changed.

Movability and the seat, run `gate1b` (same build): `zxr ctl … move 0.3 0.1 -0.2` →
`pos=(0.30,0.10,-1.70)`; `resize 900 600` → the client configured to 900×598 (foot rounds to its
cell grid); `spawn foot` → second plane at the fan's slot 1 (`x = 0.90, yaw = −0.35`); `focus
next` switches keyboard focus and `xdg_toplevel` activation. The head ray from `SIMULATED_ROTATE`
drives `wl_pointer.motion` across the planes (the gaze pointer of spec §8's R0 floor).

The runtime-call histogram (the other workstream's Phase-0 instrumentation, present in the same
binary) puts the per-tick IPC cost on this host at ≈ 240 µs across ~11 calls — `xrWaitFrame`
excluded, since it is the throttle. Per-call means are 12–51 µs; the acquire/wait/release
triple for two swapchains is the largest share. This is the number the embedded budget will
compare against on device (spec §5a's case for one batched `xrLocateSpaces`).

## 3. Gate 2 — dmabuf, explicit sync, bounded buffering

Run `r2`: foot plus `vkcube --wsi wayland --present_mode 1` (Vulkan WSI on RADV, MAILBOX: the
client renders as fast as it can and the compositor takes the latest buffer). `--present_mode 0`
(IMMEDIATE) is refused by Mesa's Wayland WSI without `tearing-control` — the mailbox case is the
fast-client case.

```
frames=1001  missed_deadlines=0  gpu_us_mean=36  gpu_us_max=62
dmabuf_imports=4  dmabuf_cpu_copies=0
acquire_syncobj=245752  acquire_implicit=0
commits=245775  buffers_released=9970
retention_frames_mean_x100=200  retention_frames_max=2
stale_texture_draws=0   (16.7 s; zxr 31 % of one core, vkcube 57 %)
```

Read: the swapchain's four `wl_buffer`s were imported once each (one `VkImage` per buffer, spec
§6.2) and never again; every one of the 245 752 commits carried a `linux-drm-syncobj-v1` acquire
point that became a calloop eventfd blocker before the commit was applied (spec §6.3) — the
implicit-fence path was never taken by this client. The compositor held at most one buffer per
surface per frame slot (retention max 2 frames = the two slots), released after that slot's
fence completed (spec §6.5); buffers the client replaced before a frame sampled them were
released by smithay at replacement. The client outran composition by ≈ 245× and nothing
queued: bounded buffering holds by construction.

**Cost of the fast client, recorded for the budget:** 14.7 k commits/s cost zxr ≈ 31 % of a
desktop core — about 21 µs per commit: the smithay commit path plus inserting and removing one
calloop source per acquire point. This is the price of doing acquire on the loop as fds rather
than blocking; a mailbox client that outruns the compositor by two orders of magnitude is the
pathological case and it degraded nothing (0 missed). On device the same path is charged to the
CPU budget; whether a per-commit source is the right granularity, or a persistent per-surface
source, is a question for the M1 budget pass — the mechanism (eventfd + calloop) is right, the
allocation pattern may not be.

The feedback table: 16 (format, modifier) pairs from the device for ARGB/XRGB/ABGR/XBGR8888
(`sampled_modifiers` over `VK_EXT_image_drm_format_modifier` properties), the main device being
`/dev/dri/renderD128`. vkcube's buffers arrived with a device modifier and imported with it —
no linear fallback was asked for.

## 4. Gate 3 — churn

Run `r3` (foot, vkcube MAILBOX, `gtk3-demo --run menus`), then run `r4b`'s GTK step:

```
resize 900 600     → 0* 900x598  (configure honoured)
key 68 (F10)       → popups=1    (GtkMenu = xdg_popup with an xdg_positioner; unconstrained
key 108 (Down) ×2  → popups=3     against the plane's 1920x1080 target, smithay's PopupManager)
kill -9 vkcube while it commits at 14.7 k/s with dmabufs in flight
                   → plane removed; toplevels_unmapped=1; stale_texture_draws=0; fences_outstanding=2
kill -9 foot (shm) → plane removed; focus handed to the survivor (focus_changes=5)
kill -9 gtk3-demo with its menu open (r4b)
                   → both toplevels and the popup gone; stale_texture_draws=0
frames=1066  missed_deadlines=0  (r3)   /   frames=1185  missed_deadlines=0  (r4b)
```

No unresolved wait: `fences_outstanding` never exceeds the two frame slots and every submitted
fence was waited at its slot's next use. No stale texture: a destroyed `wl_buffer` destroys its
`VkImage` (`buffer_destroyed`) after the frame that held it completed, and the draw list is
rebuilt from live surfaces each tick — the counter for "drew a surface whose texture is gone"
stayed at 0 through every kill. No stall: 0 missed deadlines across all churn runs.

## 5. Gate 4 — Xwayland through xwayland-satellite

Run `r4b`: `--xwayland :9 --spawn "xterm -e sh -c 'cat > FILE'"`.

```
xwayland-satellite 0.8.2 spawned (display :9); warns: no fractional-scale, no xdg-activation,
  no primary selection — all optional (research/59 §9a)
xterm → 884x556 plane; xwayland_toplevels=1 (the toplevel's client is the satellite's pid)
zxr ctl … type "gate four x11 via satellite"; key 28
FILE: "gate four x11 via satellite"
dmabuf_imports=8  acquire_implicit=8  acquire_syncobj=0   (Xwayland's glamor buffers are
                                                           dmabufs with implicit sync)
```

Satellite's fatal set (`xdg_wm_base` 2–6, `wl_compositor` 4–6, `wl_subcompositor`, `wl_shm`,
`wp_viewporter`) is met by the R0 protocol set; it sized the X screen from the virtual output's
mode (1920×1080). Keyboard input took the full path — synthesised evdev codes at zxr's seat →
`wl_keyboard` to satellite → Xwayland → xterm — and arrived intact, so the input floor's seat
path is proven for X11 clients as well as native ones (`foot -e 'cat > FILE'` received `native
wayland input ok` the same way). The fallback (`X11Wm`) was not needed.

## 6. What R0 taught (the spec's rev 2 items)

1. **Runtime events need their own source.** Session `READY` arrives before any frame can, and
   `xrWaitFrame` is only legal on a running session — an event poll bound to ticks deadlocks with
   a black mirror. A 5 ms calloop timer polls `xrPollEvent` until the session runs, 250 ms after.
   Every OpenXR application polls events each loop iteration; the loop-shape ruling is unaffected.
2. **Block the handled signals before any thread exists.** calloop's `Signals` source blocks
   them on the loop thread only; the wait thread (and Mesa's workers) spawned earlier did not
   inherit the block, so SIGTERM could reach a thread without the signalfd and kill the process
   before the journal was written (`exit=143`). `pthread_sigmask` at startup, undone in the
   child's `pre_exec`, fixed it.
3. **Teardown order is a contract.** With `xr` declared before `renderer`, the session (and its
   swapchain images) dropped before the renderer destroyed its views of those images — a SIGSEGV
   on exit (`exit=139`) in the gate-4 session. Rev 2: `xrRequestExitSession` and drive the state
   machine to `STOPPING → xrEndSession → EXITING` (so the wait thread is parked on the handshake,
   not inside the runtime), idle the device, release held buffers, destroy textures, then
   renderer before `xr`. Verified: `STOPPING → IDLE → EXITING`, `exit=0`.
4. **Both acquire paths are real.** Vulkan clients on RADV use `linux-drm-syncobj-v1`;
   Xwayland's glamor buffers use implicit sync; foot uses shm. One session exercised all three.
5. **The fast client's cost is per commit, not per frame** (§3): ≈ 21 µs per acquire on this
   host. Mechanism confirmed; granularity is an M1 budget item.
6. **Mesa's Wayland WSI has no IMMEDIATE mode without tearing-control**; MAILBOX is the fast
   client. `wp_tearing_control_v1` stays out of the R0 set (spec §10) — a headset never tears.
7. **The RSS fence needs a host caveat** (§1): 7.5 MB anon + 2.7 MB binary + RADV 4.8 MB is the
   number to compare on device; the host adds 30 MB of ICDs zxr never uses. Rev 2 states the
   fence as anon + binary + the one driver, with the host total reported alongside.

## 7. The fallback trigger, evaluated

ADR 0006 keeps wlroots as the fallback on a *structural* smithay defect. None appeared. What
smithay lacked, and what it cost: `Buffer`'s acquire/release points are private (release is by
dropping the clone — which is the correct release semantics anyway, so the exported-sync-file
alternative of spec §6.5 was not needed); no helper to enumerate a surface tree with locations
(twenty lines with `with_surface_tree_downward`); `RendererSurfaceState::view()` gives the
sub-surface offsets the desktop helpers use internally. Everything the gates needed — the
delegate states, `PopupManager` positioning, dmabuf feedback, the syncobj global and eventfd
blockers, the `X11Wm` we did not use — was present at the pinned rev. **Not triggered; the base
stands.**

## 8. Runs referenced

| run | command (after `dev-session --zxr --`) | journal facts |
|---|---|---|
| r1 | `--frames 600` | §2 |
| gate1b | `--frames 900` + `ctl move/resize/spawn/focus` | §2 |
| r2 | `--spawn "vkcube --wsi wayland --present_mode 1"`, SIGTERM at 16.7 s | §3 |
| r3 | `ctl resize`, `spawn vkcube`, `spawn gtk3-demo --run menus`, `key 68/108`, `kill -9` ×2, `focus next` | §4 |
| r4b | `--xwayland :9 --spawn "xterm -e sh -c 'cat > FILE'"`, `ctl type/key`, `spawn gtk3-demo`, `kill -9` | §4–§5 |

All runs on the host described above, concurrently with another workstream's zxr instances on
the same Monado service (Monado's multi-client compositor); the CPU percentages are therefore
upper bounds, the journal counters are per process and unaffected.
