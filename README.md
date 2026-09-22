# spatial-os

A Nix-built, NixOS-based, Wayland-based Linux XR distribution targeting many standalone VR headsets
(Oculus Quest 1, Lynx R1, Samsung Galaxy XR, Play For Dream MR, Valve Steam Frame, …). It turns
pinned vendor firmware ("donor") inputs, device definitions, and pinned sources into reproducible
flashable artifacts — building the distribution independently of the device, then qualifying and
deploying it together with an explicit, versioned hardware-adaptation bundle.

**Status:** research + architecture + initial scaffold. The `virtual-headset` VM smoke target builds
and `nix flake check` passes. Real device ports begin with a Lynx R1 hardware spike (see
[docs/architecture/design-backlog.md](docs/architecture/design-backlog.md)).

## Where things are

- [docs/](docs/README.md) — research (7 deep-dives + synthesis), architecture (5 docs + 5 ADRs),
  the red-team review, and its disposition.
- `lib/contract/` — the typed `spatial.*` device contract (NixOS-module options + assertions).
- `lib/donor/`, `lib/images/` — donor-pipeline and image-variant builders (typed stubs until the
  Lynx spike, per the design backlog).
- `modules/{os,xr,adaptation}` — the common distribution, XR runtime wiring, and per-subsystem
  adaptation backends.
- `soc/`, `families/`, `devices/` — the device → SoC-family → common decomposition.
- `references/` — git-ignored study clones, reproducible from `references/clone.sh` +
  `references/MANIFEST.json`.

## Quick start

```bash
nix flake check                                  # formatting + contract + VM smoke build
nix build .#packages.x86_64-linux.virtual-headset-vm
nix develop                                      # dev shell with donor-pipeline tooling
```

## Design rule

> Build the distribution independently of the device, but qualify and deploy it together with an
> explicit, versioned hardware dependency set.

See [docs/architecture/overview.md](docs/architecture/overview.md) for the layers, boundaries, and
non-negotiable invariants.
