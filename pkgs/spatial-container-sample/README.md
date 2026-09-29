# spatial-container-sample — the Godot client for Mura's container work

The conformance client [specs/composition.md §7.3](../../specs/composition.md) names for the
Monado C-track ([implementation-path.md §3](../../docs/architecture/implementation-path.md)):
a Godot 4.8 project that opts into `XR_EXT_spatial_container` + `_self_rendering`, logs every
container event, cycles bounded ↔ immersive, and runs unchanged on a runtime without the
extension — which Monado is today (the **C0 baseline**). The same binary becomes the C1–C4
client the day `mura-os/monado` advertises the pair.

**Provenance.** Derived from Godot's own maintainer's samples, read in this order
([research/79 §4c](../../docs/research/79-openxr-extensions-and-zxr.md)):

1. `GodotVR/godot_openxr_vendors` `samples/spatial-container-sample` — added by m4gr3d
   (Fredia Huya-Kouadio, Google; on the container spec's author list) in
   [vendors PR #536](https://github.com/GodotVR/godot_openxr_vendors/pull/536), two days after
   the engine side landed in [godot#123124](https://github.com/godotengine/godot/pull/123124)
   (merged 2026-09-08, milestone 4.8, on top of [godot#123123](https://github.com/godotengine/godot/pull/123123)
   = thirdparty OpenXR 1.1.63). **This is the base**: the robot (`3DGodotRobot.glb`,
   `godot_robot.tscn`), the action map, `start_xr.gd` (the vendors demo's copy of the docs'
   "better XR start script"), the 10-second bounded↔immersive timer, scaling content to the
   reported bounds. All MIT (`LICENSE.godot-xr-vendors`).
2. `m4gr3d/Starter-Kit-3D-Platformer` and `Starter-Kit-Racing`, branch `spatialize` — whole
   games in a container (`bounds = (2, 1, 1)`, `xr_origin.world_scale = 15 / min(bounds)`).
   Read for the scale rule; not copied (XR Tools + the vendors GDExtension + Android XR trackpad).
3. `GodotVR/spatialize` (dsnopek; m4gr3d fork) — the flat-to-XR addon whose README "OpenXR
   Spatial Containers" section is the app-side idiom: `Engine.get_singleton(
   "OpenXRSpatialContainerExtension")` → `is_enabled()` → connect `bounds_changed`. Adopted.

**Added for Mura** (`main.gd`): `SCS <event> k=v …` log lines for every event the pair defines
(a harness greps them — see §3); `B` requests the next bounds mode, `Q` quits; `--instance=N`
tints the marker cube (white/red/green/blue) so two processes are told apart in the
two-container gate (composition §7.5); `--no-cycle` stops the timer; a wireframe of the
container bounds in container space (`ext_spatial_container.adoc:278-290`: origin at the
bounds centre, +X right, +Y up, +Z front); `xr/openxr/submit_depth_buffer = true` so the
container's projection layer carries `XR_KHR_composition_layer_depth` — the input of Monado's
depth policy (composition §4.3, the C4 gate).

## 1. Build Godot master (once)

nixpkgs packages Godot 4.7-stable; the container pair is in master. Build it from a full clone
beside this repo, with every Linux dependency taken from nixpkgs' own `godot_4` recipe
(`devShells.godot` in `flake.nix` uses `inputsFrom = [ godot_4 ]`):

```sh
git clone https://github.com/godotengine/godot.git /run/media/j/tinystore/experiments/godot
git -C /run/media/j/tinystore/experiments/godot checkout 941ea1816d654e4ffeac10a260d64a8555890cf6  # = references/godot
cd /run/media/j/tinystore/experiments/mura
nix develop .#godot -c bash -c 'cd ../godot && scons platform=linuxbsd target=editor \
  builtin_openxr=yes use_sowrap=no wayland=yes x11=yes import_env_vars="$GODOT_IMPORT_ENV_VARS" -j$(nproc)'
```

- `builtin_openxr=yes` (Godot's default) uses the vendored 1.1.63 loader/headers; nixpkgs'
  `openxr-loader` is 1.1.62 at the pin and lacks the container symbols — do not pass
  `builtin_openxr=no`.
- `import_env_vars` hands the Nix compiler wrapper's `NIX_*` variables to scons (nixpkgs
  patches `SConstruct` to copy `os.environ` instead; Godot's own option does the same).
- An *editor* build runs a project with `--path` and needs no export templates.
- Output: `bin/godot.linuxbsd.editor.x86_64` (~250 MB with debug symbols; ~35 min on 28 cores).
  Check it: `bin/godot.linuxbsd.editor.x86_64 --headless --dump-extension-api /tmp/api.json
  && grep -c OpenXRSpatialContainer /tmp/api.json` (non-zero).

A reproducible `pkgs/godot` (`godot_4.overrideAttrs { src = …master… }`) is deferred until C1
exists — decider: the owner (plan A4). Until then the wrapper takes the binary from `GODOT`
(default: the path above).

## 2. Run

```sh
# terminal 1 — the dev loop: simulated-HMD Monado + zxr, Monado's mirror window on
nix run .#dev-session -- --godot
# or, inside any session with XR_RUNTIME_JSON set:
GODOT=/path/to/godot.linuxbsd.editor.x86_64 nix run .#spatial-container-sample -- --instance=1
```

The wrapper copies the project to `$XDG_CACHE_HOME/mura/spatial-container-sample/<store-hash>/`
(the store is read-only), runs one headless `godot --import` pass there the first time a
build is used — a project only runs after the editor has imported it: `.godot/imported/` holds
the converted robot and `.godot/global_script_class_cache.cfg` registers `StartXR`; without it
`main.gd` fails to parse and the window stays black — and then runs
`godot --path … --xr-mode on`, so a missing runtime is an error, not a silent flat window.

**What to expect today (C0, Monado without the pair)** — recorded 2026-09-29 against
nixpkgs-xr's Monado 25.1.0 (simulated HMD) with zxr in the compositor slot:

```
[godot] OpenXR: Max spatial container count:  0          ← Godot probes; the runtime lacks the ext
[godot] SCS ext absent runtime=Monado(XRT)_by_Collabora_et_al_'GIT-NOTFOUND'
[godot] SCS session begun
[godot] SCS session visible
[godot] SCS session focused
```

Two windows appear on the desktop: Monado's mirror and Godot's own window (Godot blits the
left eye to it). The mirror shows zxr until Godot's session reaches FOCUSED, then **the robot
replaces zxr** — today's Monado composites the focused session's projection layers only, and
zxr drops to VISIBLE. That replacement is the behaviour the C-track removes: from C2 on, the
same run shows the robot as a bounded container *inside* zxr's scene (composition §4). No
`SCS bounds`/`visible`/`interactable` lines appear — they need the runtime. Without a bounded
container the content is parked 1 m ahead at chest height (`IMMERSIVE_CONTENT_POSITION`);
the vendors sample leaves it at the origin, which is the container's centre when a runtime
hosts one but the viewer's feet when none does. `Q` quits with `SCS quit`.

## 3. The `SCS` lines and the composition §7 gates

| line | when | gate |
|---|---|---|
| `SCS ext absent\|present runtime=… suggested_bounds=…` | startup | §7.6 (a container session exists) |
| `SCS caps supported_bounds_modes=[…] mode=N visible=… interactable=… bounds=…` | session begun, ext present | §7.3 (`XrSystemSpatialContainerPropertiesEXT` → Godot's `get_supported_bounds_modes`) |
| `SCS visible true\|false` / `SCS visible_request_denied` | `XrEventDataSpatialContainerVisibleChangedEXT` / `…RequestDeniedEXT` | §7.3, §7.4 (zxr shows/hides through the seam) |
| `SCS interactable true\|false` | `…InteractableChangedEXT` | §7.4 (focus-on-commit designated by zxr) |
| `SCS bounds mode=0\|1 infinite=… bounds=(x, y, z)` | `…BoundsChangedEXT` | §7.3, §7.4 (bounds from the seam; immersive = the fullscreen game) |
| `SCS request_bounds_mode mode=N accepted_call=…` / `SCS bounds_mode_request_denied` | timer or `B` | §7.3 (request-and-deny, `ext_spatial_container.adoc:690-705`) |
| `SCS closed` | `…ClosedEXT` → the sample quits | §7.4 (close from the window menu) |
| two instances, `--instance=1` and `--instance=2`, overlapping bounds | — | §7.5 (depth interleave; both submit depth) |

## 4. Files

`project.godot` (settings above), `main.tscn` (XROrigin3D + camera, light, `Content/` with the
robot, the marker cube and the bounds box), `main.gd` (Mura), `start_xr.gd` (vendors, MIT,
header added), `godot_robot.tscn` + `3DGodotRobot.glb` + palette PNG and their `.import`
files (vendors, MIT), `openxr_action_map.tres` (vendors), `LICENSE.godot-xr-vendors`.
