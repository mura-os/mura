# Spike runbook: share-the-app via waypipe (two-node VM test)

**Status: written, deliberately NOT executed.** This is a research/architecture phase; the
share-the-app tier's mechanics are established from source study in
[docs/research/19-wayland-proxying.md](../docs/research/19-wayland-proxying.md). This runbook exists
so the empirical confirmation can be run on demand later, reproducibly, without redesigning it.

## What it proves (when run)

Mode 4 of the sharing taxonomy ("share the app"): an app running on one machine (`appHost`, running
`foot` under `waypipe server`) appears as a **real `wl_surface`** in a compositor on another machine
(`xrHost`, headless sway + `waypipe client`) — no pixel capture, the Wayland protocol itself is
forwarded across a genuine machine boundary (the two waypipe unix sockets bridged over TCP with
socat, standing in for ssh / USB-gadget CDC-NCM).

## How to run

```bash
nix build --impure -f spikes/waypipe-vm/test.nix -o spikes/result-waypipe
```

Asserts `foot`'s toplevel appears in sway's tree (`swaymsg -t get_tree` / `app_id == "foot"`) and
captures a grim screenshot plus both waypipe logs into the test output.

## Execution log

- Attempt 1: failed in test-driver typecheck — `copy_from_vm` deprecated → renamed to
  `copy_from_machine` (fixed in test.nix).
- Attempt 2: VMs booted, sway started (wayland socket appeared), but `swaymsg` failed with
  "Unable to retrieve socket path" — swaymsg needs `SWAYSOCK`, which cannot be derived from
  `WAYLAND_DISPLAY` (fixed in test.nix: resolve `SWAYSOCK=$(ls /tmp/xdg/sway-ipc.*.sock)` after
  waiting for the IPC socket).
- Not rerun since: scoped out of the research phase per project owner direction. The fixes above are
  in the committed test; next run starts from cached VM closures (~3-5 min).

## Design notes captured from the attempts

- Headless sway works in a stock NixOS VM with `WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
  WLR_RENDERER=pixman` and no logind session — the recipe for compositor CI generally.
- foot renders via wl_shm/pixman, so the test exercises waypipe's shm diff+compress path with no GPU
  in either guest; a dmabuf-path variant would swap foot for a GL client and needs virtio-gpu.
- waypipe's split-socket mode (`waypipe --socket X client` / `waypipe --socket Y server -- cmd`)
  bridges over any reliable byte stream; socat TCP is the minimal stand-in for the USB-gadget
  (CDC-NCM) transport discussed in doc 19.
