# 05 — XR Userspace: Packaging, Configuration, and Integration

Research for Mura (Nix/NixOS-based Wayland XR distro for standalone headsets).
Focus: what the build system must **build, ship, wire together, and make configurable** —
not runtime algorithm internals.

Sources studied (local clones under `references/`): monado, wivrn, stardustxr-server,
nixpkgs-xr, envision. Web: <https://vronlinux.org/docs/distros/nixos/>, OpenXR loader spec
<https://registry.khronos.org/OpenXR/specs/1.1/loader.html>. File paths below are relative
to each repo root.

---

## 1. Project purpose

- **Monado** — the FOSS OpenXR runtime ("XRT"). Self-described as a "runtime construction
  kit" of independent modules (drivers, compositor, IPC, state trackers) assembled into
  final "targets" (`doc/understanding-targets.md`). It is the OpenXR runtime everything
  else here layers on.
- **WiVRn** — a Monado-*derived* streaming runtime: an Android client APK runs on the
  standalone headset, a Linux `wivrn-server` (embedding Monado's IPC/compositor stack)
  runs on the host and encodes/streams frames. The single most relevant evidence of what
  a Monado-derived runtime shipping to ARM/Android headsets needs (`README.md`,
  `CMakeLists.txt`).
- **StardustXR server** — a 3D "display server" for XR: an OpenXR *client* of Monado/WiVRn
  that hosts its own client apps over a custom IPC protocol; the XR analog of a Wayland
  compositor (`README.md`, `Cargo.toml` description: "Stardust XR reference display
  server").
- **nixpkgs-xr** — nix-community overlay providing bleeding-edge builds of the XR stack
  on top of nixpkgs, with daily automated source bumps (`README.md`, `flake.nix`,
  `nvfetcher.toml`).
- **Envision** — GTK4 GUI "Orchestrator for the free XR stack" (`meson.build`): clones and
  builds Monado + trackers + OpenVR-compat from git into per-profile prefixes, and writes
  the config files that activate them. Read here as a de-facto encoding of the working
  Linux XR dependency graph.

## 2. Repository / build architecture

### 2.1 Monado (CMake)

Single CMake project, C11/C++20 (`CMakeLists.txt`). Three tiers of options, all listed in
the top-level `CMakeLists.txt` and printed at configure time:

- `XRT_HAVE_*` — dependency availability (auto-detected, force-off-able): Vulkan, OpenGL/
  GLES/EGL, Wayland (`XRT_HAVE_WAYLAND`, and `XRT_HAVE_WAYLAND_DIRECT` requiring
  wayland-protocols ≥ 1.22 for DRM leasing), XCB/Xlib/XRandR, libudev, systemd, D-Bus,
  OpenCV, ONNXRuntime, libusb, hidapi, GStreamer, etc.
- `XRT_MODULE_*` — component selection: `XRT_MODULE_IPC`, `XRT_MODULE_COMPOSITOR_MAIN`
  (the Vulkan compositor), `XRT_MODULE_COMPOSITOR_NULL`, `XRT_MODULE_MONADO_CLI/GUI`,
  `XRT_MODULE_MERCURY_HANDTRACKING` (needs OpenCV+ONNX), `XRT_MODULE_CONSTELLATION_TRACKING`
  (needs full Ceres).
- `XRT_FEATURE_*` — runtime shape: **`XRT_FEATURE_SERVICE`** selects out-of-process mode
  (`monado-service` + thin `libopenxr_monado.so` IPC client) vs in-process (everything in
  the .so); `XRT_FEATURE_SERVICE_SYSTEMD` enables systemd socket activation;
  `XRT_FEATURE_SLAM`; plus ~60 per-OpenXR-extension toggles
  (`XRT_FEATURE_OPENXR_HAND_TRACKING_EXT`, interaction-profile toggles, layer toggles).
- `XRT_BUILD_DRIVER_*` — one flag per device driver (~35: `VIVE`, `WMR`, `RIFT_S`,
  `SURVIVE`, `STEAMVR_LIGHTHOUSE`, `PSVR2`, `XREAL_AIR`, `OHMD`, `SIMULATED`, ...). The
  driver list is closed at build time: "All drivers must be listed in here to be included
  in the generated header" — `AVAILABLE_DRIVERS` list, though "You can set this from a
  superproject to add a driver" (`CMakeLists.txt` ~line 470).

Targets that a distro installs (`src/xrt/targets/`, `doc/understanding-targets.md`):

| Artifact | Target dir | Role |
|---|---|---|
| `libopenxr_monado.so` | `targets/openxr/` | OpenXR runtime entry, loaded by the OpenXR loader; deliberately **unversioned SONAME** in the normal libdir (`doc/packaging-notes.md`) |
| `monado-service` | `targets/service/` | out-of-process service (when `XRT_FEATURE_SERVICE`) |
| `libmonado.so` | `targets/libmonado/` | versioned management API for dashboards (`doc/packaging-notes.md`) |
| `monado-cli`, `monado-gui` | `targets/cli|gui/` | utilities, link drivers in-process |
| SteamVR plugin, OpenVR `vrclient.so` | `targets/steamvr_drv/`, `targets/openvr/` | optional compat |

OpenXR manifest: `targets/openxr/CMakeLists.txt` generates `share/openxr/1/openxr_monado.json`
(runtime path relative to manifest by default: `../../../${CMAKE_INSTALL_LIBDIR}`, or
absolute with `XRT_OPENXR_INSTALL_ABSOLUTE_RUNTIME_PATH`; manifest also carries
`MND_libmonado_path`). `XRT_OPENXR_INSTALL_ACTIVE_RUNTIME=ON` additionally installs a
symlink `/etc/xdg/openxr/1/active_runtime.json` → the manifest
(`targets/openxr/active_runtime.cmake`) — exactly the file the OpenXR loader searches in
`XDG_CONFIG_HOME`, then `XDG_CONFIG_DIRS` (`/etc/xdg`), with `active_runtime.<arch>.json`
checked before `active_runtime.json` (loader spec §"Linux Active Runtime Location";
override via `XR_RUNTIME_JSON` env var).

systemd wiring: `targets/service/monado.in.service` + `monado.in.socket` (user units,
`ListenStream=%t/monado_comp_ipc`, socket-activated, `ConditionUser=!root`), installed
when `XRT_INSTALL_SYSTEMD_UNIT_FILES=ON`. Socket name configurable via CMake cache var
`XRT_IPC_MSG_SOCK_FILENAME` (default `monado_comp_ipc`).

Monado also carries its own `flake.nix` (dev shell, depends on `nixpkgs-xr` as an input)
— evidence the projects already interlock at the Nix level.

### 2.2 WiVRn (CMake + Gradle; Monado via FetchContent)

Host side: CMake ≥ 3.28, C++23. Monado is **not a submodule and not a fork repo**: it is
fetched at configure time by `FetchContent_Declare(monado GIT_REPOSITORY
https://gitlab.freedesktop.org/monado/monado.git GIT_TAG ${MONADO_REV} PATCH_COMMAND
patches/apply.sh ...)` where the pin is a plain git SHA in the top-level file
**`monado-rev`**, and a **series of 11 patches** in `patches/monado/*.patch` is applied
with `git am` (`CMakeLists.txt` lines 268–278, `patches/apply.sh`). Patches are small
integration changes (extern socket fd, blend-mode selection, steamvr_lh fixes), not a
divergent fork. WiVRn then builds its own service around Monado's libraries
(`server/target_instance_wivrn.cpp`, `server/driver/*` implementing `xrt_device`s fed by
the network).

Key build options: `WIVRN_BUILD_CLIENT/SERVER/DASHBOARD/WIVRNCTL`, encoder selection
(`WIVRN_USE_NVENC/VAAPI/VULKAN_ENCODE/X264`), `WIVRN_USE_PIPEWIRE`, feature passthroughs
mirroring Monado's (`WIVRN_FEATURE_STEAMVR_LIGHTHOUSE`, `WIVRN_FEATURE_SOLARXR`), and
`OVR_COMPAT_SEARCH_PATH` (a baked-in search list for OpenComposite/xrizer)
(`CMakeLists.txt` lines 35–84).

Installed host artifacts (`server/CMakeLists.txt` lines 279–325): `wivrn-server`,
systemd **user** unit `wivrn.service` (heavily sandboxed: `NoNewPrivileges`,
`ProtectSystem=strict`, ... — `server/dist/wivrn.service.in`), firewalld service XML, an
arch-suffixed OpenXR manifest `share/openxr/1/openxr_wivrn.<arch>.json` from
`server/dist/openxr_manifest.in.json`, plus the runtime .so and `libmonado`.

Runtime-manifest activation is **dynamic, not package-time**: on connect, the server
backs up `$XDG_CONFIG_HOME/openxr/1/active_runtime.json` (suffix `.wivrn-backup`) and
symlinks it to its own manifest, restoring on exit; it also rewrites
`openvrpaths.vrpath` for the OpenVR compat layer (`server/active_runtime.cpp`). Config is
JSON at `$XDG_CONFIG_HOME/wivrn/config.json`; pairing state in `wivrn/known_keys.json`
(`server/driver/configuration.cpp`).

Client side (the standalone-headset half): Gradle + NDK (`build.gradle`:
`applicationId org.meumeu.wivrn`, `minSdkVersion 29`, `targetSdkVersion 32 // for Oculus
Store`, `abiFilters 'arm64-v8a'`, NDK pinned `29.0.14206865`), CMake invoked through
externalNativeBuild with product flavors including `oculus`. The client is a native
OpenXR *application* against the headset vendor's own Android OpenXR runtime — WiVRn does
not replace the headset OS; it rides on it. The dashboard installs the matching APK on
the headset via adb, downloading it from GitHub releases keyed to the exact
`wivrn::git_commit` (`dashboard/apk_installer.cpp`, `dashboard/adb.cpp`) — client/server
are version-locked as a pair.

### 2.3 StardustXR server (Cargo)

Rust 2024 workspace, `cargo` only; version 0.52.0 (`Cargo.toml`). Rendering via **Bevy
0.16** + `bevy_mod_openxr`; notable pinning hygiene problem for a distro: `[patch.crates-io]`
redirects `bevy_mod_openxr`, `wgpu` (5 crates), `bevy_pbr`/`bevy_render` etc. to personal
git forks at branches/revs (`Cargo.toml` lines 48–60), so builds depend on mutable
GitHub branches (locked only through `Cargo.lock`). Links against `openxr` crate 0.19 →
needs `openxr-loader` at runtime; nix packaging in-tree confirms the runtime closure:
`nix/stardust-xr-server.nix` builds with `buildRustPackage` + `cargoLock.lockFile`,
`buildInputs = [vulkan-loader openxr-loader wayland alsa-lib]` and patchelf's rpaths for
vulkan/openxr/xkbcommon.

Client connection model: server binds per-instance filesystem sockets ("fs binds") named
by `STARDUST_INSTANCE` (via `stardust-xr-protocol::dir::find_free_instace`), exposes a
D-Bus name `org.stardustxr.Server.<instance>`, then runs a **startup script**
(`~/.config/stardust/startup`) or restores a saved session from
`~/.local/state/stardust/<id>`; launched clients inherit `STARDUST_INSTANCE`,
`XDG_CURRENT_DESKTOP=Stardust`, and `FLAT_WAYLAND_DISPLAY` (copied from the host
`WAYLAND_DISPLAY`) (`src/main.rs` lines 209–260, `src/session.rs`). **This revision
contains no in-tree Wayland compositor** (no smithay/wayland-server dependency; the only
"wayland" is Bevy's windowing feature) — 2D Wayland app integration is delegated to
client apps (e.g. Flatland, referenced by `flake.nix` for VM tests). A "Stardust session"
is therefore: OpenXR runtime service → `stardust-xr-server` (OpenXR client) → stardust
clients over its own IPC.

### 2.4 nixpkgs-xr (Nix overlay)

Flake outputs: `overlays.default`, per-package outputs, and one NixOS module
(`flake.nix`, `nixos/default.nix`). Structure (`pkgs/overlay.nix`):

1. `xrSources = callPackage ../_sources/generated.nix` — **nvfetcher**-generated pins
   (rev + sha256 + extracted `Cargo.lock`s) for ~20 projects (`nvfetcher.toml`,
   `_sources/generated.json`).
2. New packages via nixpkgs' `by-name-overlay` (`pkgs/by-name/`: lovr, xrbinder,
   xr-chaperone, vapor, index_camera_passthrough, ...).
3. `pkgs/overrides/*.nix` — **overrideAttrs on nixpkgs' own recipes** to swap in git
   sources: monado, wivrn, envision, libsurvive, opencomposite, oscavmgr, wayvr, xrizer.

Standout mechanism: the `wivrn-monado` nvfetcher entry scrapes WiVRn's `monado-rev` file
from GitHub (`src.webpage = ".../raw/.../monado-rev"`, `src.regex = "(\\w+)"`) and the
wivrn override rebuilds the pinned+patched Monado with `applyPatches` reusing nixpkgs'
patch list (`nvfetcher.toml`, `pkgs/overrides/wivrn.nix`) — the WiVRn↔Monado pin is
honored inside Nix without vendoring.

Update automation: GitHub Actions cron `0 0 * * *` runs nvfetcher
(`.github/workflows/nvfetcher.yaml`); Renovate maintains flake inputs (`renovate.json`);
builds cached at nix-community cachix. The NixOS module (`nixos/default.nix`) does only
two things: adds the overlay and the cachix substituter (`nixpkgs.xr.enable`, default
true once imported). **The actual service modules live upstream in nixpkgs**:
`nixos/modules/services/hardware/monado.nix` (`services.monado.enable/defaultRuntime/
highPriority`) and `nixos/modules/services/video/wivrn.nix` (`services.wivrn`, with
declarative `config` → wivrn JSON, `highPriority` via `security.wrappers` cap_sys_nice,
Monado env defaults reproduced from `monado.in.service`)
(<https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/services/video/wivrn.nix>).
Under NixOS, `services.monado.defaultRuntime` materializes
`/etc/xdg/openxr/1/active_runtime.json`; WiVRn instead flips the user symlink at runtime
(vronlinux NixOS page).

### 2.5 Envision (Meson + Cargo, builds others from git)

Meson wraps cargo for the GUI itself (`meson.build`); the interesting part is what it
builds *for you*: `src/builders/build_{monado,libsurvive,basalt,mercury,opencomposite,
openhmd,xrizer,vapor}.rs` each clone a default repo/branch (`src/util/git_repos.rs`:
monado main, mateosss/basalt fork, cntools/libsurvive, OpenComposite `openxr` branch,
thaytan/OpenHMD `rift-room-config` branch, xrizer main) and run CMake/cargo installs into
a per-profile prefix under `~/.local/share/envision/prefixes/<profile>` with rpath set to
the prefix lib dir (`src/builders/build_monado.rs`). Profiles (`src/profiles/*.rs`:
lighthouse, wmr, survive, openhmd, simulated) = XR service type (only Monado,
`src/profile.rs`) + feature toggles (libsurvive / basalt / openhmd) + OpenVR compat
module (OpenComposite or xrizer) + an **environment-variable map** (e.g. lighthouse
profile sets `XRT_COMPOSITOR_SCALE_PERCENTAGE=140`, `XRT_COMPOSITOR_COMPUTE=1`,
`U_PACING_APP_USE_MIN_FRAME_PERIOD=1`, `LD_LIBRARY_PATH=<prefix>/lib`;
`src/profiles/lighthouse.rs`) + a `LighthouseDriver` enum (libsurvive vs Monado's
`steamvr_lh` vs vive). Activation = writing `~/.config/openxr/1/active_runtime.json`
(symlink to the profile's `openxr_monado.json`, `.envision.bak` backup) and
`~/.config/openvr/openvrpaths.vrpath` (`src/file_builders/active_runtime_json.rs`,
`openvrpaths_vrpath.rs`). Mercury hand tracking = `scripts/build_mercury.sh`, which
git-LFS-clones `gitlab.freedesktop.org/monado/utilities/hand-tracking-models` into
`~/.local/share/monado/hand-tracking-models`. Envision also checks for the `xr-hardware`
distro package (udev rules) and offers `setcap CAP_SYS_NICE=eip` on the service binary
(`src/depcheck/common.rs` line 357, `src/ui/main_view.rs`).

**Implied dependency graph of a working stack** (Envision's encoding):

```
openxr app ── OpenXR loader ── active_runtime.json ── libopenxr_monado.so (IPC client)
                                                          │ $XDG_RUNTIME_DIR/monado_comp_ipc
openvr app ── openvrpaths.vrpath ── OpenComposite/xrizer ─┤
                                                     monado-service
                        ┌───────────────┬────────────────┼──────────────┬───────────┐
                  libsurvive        steamvr_lh      Basalt (SLAM,   Mercury HT   udev rules
                  (lighthouse)   (SteamVR blobs)  VIT_SYSTEM_LIBRARY (ONNX models (xr-hardware)
                                                   _PATH=libbasalt.so)  via LFS)
```

## 3. Device abstraction model

- Unit of device support = an in-tree Monado driver implementing `xrt_device`, discovered
  either by USB VID/PID prober entry or an `xrt_auto_prober`, and **registered in a
  static list compiled into the target**: `src/xrt/targets/common/target_lists.c`
  (`doc/writing-driver.md`).
- **There is no stable out-of-tree driver ABI.** Monado explicitly: "Monado is intended
  to not expose any external API other than the OpenXR API: the xrt_iface are subject to
  change as required... those writing drivers... are encouraged to upstream as much as
  possible" (`doc/writing-driver.md`). The supported extension points are (a) upstream
  the driver, (b) build Monado as a superproject/toolkit with your own target and lists
  (the `AVAILABLE_DRIVERS` append hook in `CMakeLists.txt`), or (c) pin+patch like WiVRn.
  For Mura, a device port realistically means **a pinned Monado rev + patch set or
  a superproject target**, per device — WiVRn's `monado-rev` + `patches/monado/` is the
  proven pattern.
- The one dlopen'd plugin seam that *does* exist: SLAM/VIT trackers. `t_tracker_slam`
  dlopens `libbasalt.so` (or `$VIT_SYSTEM_LIBRARY_PATH`) through a small "VIT" C ABI
  (`src/xrt/auxiliary/tracking/t_tracker_slam.cpp` lines 54–87, `t_vit_loader.c`), so
  SLAM implementations are swappable at runtime without rebuilding Monado.
- Runtime (not build-time) driver selection/config:
  - env vars via Monado's `u_debug`/option system (`XRT_COMPOSITOR_*`, `STEAMVR_LH_ENABLE`,
    `U_PACING_*`, `P_OVERRIDE_ACTIVE_CONFIG`, per-driver log levels) — this is the main
    config surface, which Envision models as per-profile env maps and NixOS models as
    `systemd.user.services.monado.environment` (vronlinux NixOS page).
  - user config file `$XDG_CONFIG_HOME/monado/config_v0.json` (tracking overrides,
    remote mode; `src/xrt/auxiliary/util/u_config_json.c`).
  - udev rules: not shipped in the Monado tree; delegated to the separate freedesktop
    `xr-hardware` package (Envision `src/depcheck/common.rs`), which grants user access
    to HMD HID/USB nodes.
  - data files: hand-tracking models at `$XDG_DATA_HOME/monado/hand-tracking-models`
    (`u_file_get_hand_tracking_models_dir`, `src/xrt/auxiliary/util/u_file.c`).
- What a new headset needs, concretely: a Monado driver (in-tree flag or patch set),
  its `XRT_BUILD_DRIVER_*` enabled, udev rules for its USB/HID IDs, any tracker plugin
  (Basalt build for SLAM devices), device calibration data path, and env-var defaults —
  i.e. exactly the per-device option bundle Mura's `modules/xr/` must express.
- WiVRn sidesteps hardware drivers entirely: its "devices" are network-fed `xrt_device`s
  (`server/driver/wivrn_controller.cpp` etc.), and per-headset differences live in the
  Android client (`client/hmd_traits.cpp`, gradle flavors).

## 4. Vendor blob / donor firmware handling

- **Mercury hand tracking**: ONNX model weights from
  `gitlab.freedesktop.org/monado/utilities/hand-tracking-models`, fetched out-of-band by
  git LFS (`scripts/get-ht-models.sh` in monado, `scripts/build_mercury.sh` in envision)
  — never packaged into Monado itself; a distro must ship them as a separate fixed-output
  package into the XDG data path.
- **`steamvr_lh` lighthouse driver** (`XRT_BUILD_DRIVER_STEAMVR_LIGHTHOUSE`): loads the
  **proprietary SteamVR driver binaries** from a Steam install; vronlinux documents it as
  the recommended lighthouse path over FOSS libsurvive ("better results, despite being
  closed source & requiring SteamVR"). An appliance distro can't assume Steam — this is a
  desktop-only crutch.
- Per-device factory calibration (Vive/Index JSON, WMR config) is read from the device
  at probe time by drivers (`XRT_MODULE_AUX_VIVE`), not shipped — no donor-firmware
  handling exists anywhere in this stack.
- WiVRn client links vendor OpenXR loaders per flavor (Oculus/Pico gradle flavors,
  `targetSdkVersion 32 // for Oculus Store`, `build.gradle`) — on Android the vendor
  runtime *is* the blob, taken from the headset OS.
- ONNXRuntime itself (`XRT_HAVE_ONNXRUNTIME`) and CUDA/NVENC (WiVRn `WIVRN_USE_NVENC`)
  are the other closed/semi-closed deps a distro must decide on.

## 5. Kernel strategy (what these components require from kernel/userspace)

- **DRM leasing is the load-bearing display mechanism** for PC-style direct mode:
  Monado's compositor backends (`src/xrt/compositor/main/`) include
  `comp_window_direct_wayland.c` built on the `wp_drm_lease_v1` Wayland protocol (gated
  by `XRT_HAVE_WAYLAND_DIRECT`, wayland-protocols ≥ 1.22), `comp_window_direct_randr.c` /
  `comp_window_direct_nvidia.c` (X11 leases), and `comp_window_vk_display.c`
  (`VK_KHR_display` — Monado owns the DRM device directly, **no display server needed**:
  `VK_USE_PLATFORM_DISPLAY_KHR` set whenever Vulkan && !Android, `CMakeLists.txt` line
  569). For an embedded headset where the panel is the only display,
  `comp_window_vk_display` is the natural distro choice: kernel must expose panels as
  DRM connectors (with `drm.edid_firmware`/non-desktop quirks as needed).
- udev: `XRT_HAVE_LIBUDEV` required on Linux for probing (`find_package(udev REQUIRED)`),
  plus the `xr-hardware` rules package for unprivileged access.
- V4L2 (`XRT_HAVE_V4L2` forced TRUE on Linux) for tracking cameras; libusb/hidapi for
  everything else.
- Scheduling: both Monado and WiVRn want `CAP_SYS_NICE` on the service binary (Envision's
  setcap flow; nixpkgs `services.wivrn.highPriority` via `security.wrappers`).
- WiVRn host additionally needs hardware video encode (VAAPI/NVENC/Vulkan video) and
  avahi/mDNS (`server/avahi_publisher.cpp`) plus open UDP/TCP port 9757
  (`server/dist/firewalld-wivrn.xml`).
- StardustXR: Vulkan + dmabuf import (`bevy-dmabuf`, forked wgpu for explicit sync,
  `timeline_syncobj` crate → **DRM syncobj timeline support**, i.e. recent kernels &
  drivers), xkbcommon, ALSA.
- Community pain points absorbed by kernel choice (vronlinux NixOS page): SteamVR async
  reprojection needs an out-of-tree AMD-only kernel patch (irrelevant if SteamVR is
  rejected); sandboxed apps (pressure-vessel) need `/nix` and the manifest path visible.

## 6. Image assembly and flashing

Not applicable to any studied project — all assume an existing host OS. What they assume,
i.e. the contract Mura's image must provide:

- A **systemd user session** with D-Bus: Monado/WiVRn ship user units; WiVRn starts apps
  via `org.freedesktop.systemd1` transient units (`server/start_systemd_unit.cpp`,
  `dbus/`); Stardust claims a session D-Bus name.
- XDG base dirs (`XDG_RUNTIME_DIR` for the IPC socket, `XDG_CONFIG_HOME` for
  active_runtime/config, `XDG_DATA_HOME` for models/state).
- An OpenXR **loader** package (separate Khronos project; `openxr-loader` in nix) — apps
  link the loader, never the runtime (`doc/packaging-notes.md`).
- WiVRn client deployment today is APK-sideloading via adb from GitHub releases
  (`dashboard/apk_installer.cpp`) — i.e. the "flashing" story for the headset side is
  entirely the vendor's Android; Mura replacing the headset OS is precisely the
  gap none of these projects cover.

## 7. Update mechanism / versioning / ABI

- **Monado**: CalVer-ish project version (25.1.0, `CMakeLists.txt`), gitlab main-branch
  development, releases sparse — every consumer studied pins a **git rev**, not a release
  tarball. ABI facts a distro must respect: `libopenxr_monado.so` unversioned by design;
  `libmonado.so` versioned; the **service↔client IPC protocol has no stability
  guarantee** (`XRT_FEATURE_CLIENT_WITHOUT_SERVICE` exists only for building both halves
  with compatible options: "do not affect the IPC ABI", `CMakeLists.txt` line 328) ⇒
  `monado-service` and `libopenxr_monado.so` must always update **atomically from the
  same build** — trivially guaranteed by a Nix closure, painful anywhere else.
- **WiVRn**: server and APK are locked to the same git commit (APK fetched by
  `wivrn::git_commit`, `dashboard/apk_installer.cpp`); protocol compatibility enforced at
  connect. Monado pin advanced by editing `monado-rev` + rebasing 11 patches.
- **nixpkgs-xr**: daily nvfetcher cron bumps every source; Renovate bumps flake inputs;
  CI builds + cachix publishes. Effectively "rolling git HEAD, but pinned and cached at
  each step".
- **Envision**: `git pull` on build (`pull_on_build`), branch-following, no pinning —
  updates are whatever upstream main is today.
- **StardustXR**: crates.io versions for protocol crates (0.52.x) but git-branch patches
  for the graphics stack; releases via cargo version bumps.

## 8. Reproducibility properties

- **nixpkgs-xr: good.** Every source in `_sources/generated.json` carries rev + sha256;
  Cargo.locks extracted and their git deps hashed (`cargo_lock`/`extract` in
  `nvfetcher.toml`); the WiVRn→Monado cross-pin is machine-read from `monado-rev`
  (`wivrn-monado` entry) so the overlay can't drift from what WiVRn tested. Flake inputs
  locked. Caveats: it tracks *heads* daily (bleeding-edge by mission), and correctness
  depends on nixpkgs' underlying recipes.
- **WiVRn: good given network access at configure.** Exact SHA in `monado-rev`, patches
  in-tree; but FetchContent downloads at configure time — under Nix this is replaced by
  the pre-fetched, patched source passed as the `monado` attr (`pkgs/overrides/wivrn.nix`,
  wivrn's own `flake.nix` does the same with `applyPatches`).
- **Envision: none.** Clones mutable branches into a mutable user prefix, `git pull` on
  rebuild; two users on the same day can get different stacks. vronlinux explicitly warns
  against Envision on NixOS ("frequently breaks... may mess with your monado.service").
  Its *profile schema* is valuable; its build mechanism is the anti-pattern.
- **StardustXR: medium.** Cargo.lock pins everything including the git-branch patches,
  but branch-based `[patch.crates-io]` entries make bumping hazardous and upstream
  reproducibility contingent on forks not force-pushing.

## 9. What Mura should adopt

1. **Monado as the system OpenXR runtime, out-of-process** (`XRT_FEATURE_SERVICE=ON`,
   `XRT_FEATURE_SERVICE_SYSTEMD=ON`): socket-activated user service + system-wide
   `/etc/xdg/openxr/1/active_runtime.json` symlink managed **declaratively by the module
   system** (the `XRT_OPENXR_INSTALL_ACTIVE_RUNTIME` install hook / nixpkgs
   `services.monado.defaultRuntime` pattern), never by runtime symlink-flipping.
2. **The WiVRn pinning pattern for device ports**: per-device Monado = upstream rev file
   + curated patch series applied at build (`monado-rev` + `patches/monado/`), realized
   in Nix via `applyPatches` like `pkgs/overrides/wivrn.nix`. This is the realistic
   "out-of-tree driver" mechanism given Monado's unstable internal ABI.
3. **nixpkgs-xr's update architecture**: nvfetcher-generated `_sources` with hashes,
   cross-pin scraping (wivrn-monado trick), scheduled bump PRs, binary cache. Reuse the
   overlay directly as a flake input where possible (Monado's own flake already does).
4. **Monado's CMake option surface as the template for `modules/xr/` options**, roughly:
   - `xr.runtime = "monado" | "wivrn"` (runtime package + manifest + service wiring);
   - `xr.monado.drivers.<name>.enable` → `XRT_BUILD_DRIVER_*` flags (build a minimal,
     per-device runtime — the flags exist precisely for this);
   - `xr.monado.compositor.backend = "vk-display" | "wayland-direct" | "window"` →
     `XRT_MODULE_COMPOSITOR_MAIN` windowing selection; vk-display for the appliance case;
   - `xr.monado.environment = { ... }` → systemd unit env (the proven config channel:
     Envision profiles ≙ vronlinux's `systemd.user.services.monado.environment`);
   - `xr.tracking.slam.package` → provides `libbasalt.so`, sets
     `VIT_SYSTEM_LIBRARY_PATH`; `xr.tracking.handModels.enable` → fixed-output
     hand-tracking-models package linked into `XDG_DATA_HOME`-visible path;
   - `xr.udevRules` (ship `xr-hardware` equivalent in-image); calibration data paths as
     declared package/state paths.
5. **Envision's profile schema as the semantic model** (service + features + compat
   module + env + driver choice) — reimplemented as Nix module options, not as its
   imperative builder.
6. **WiVRn's session/service hygiene**: sandboxed systemd user unit, firewall service
   definition, avahi publication, systemd-launched XR applications via D-Bus transient
   units — a ready pattern for "XR session launches apps".
7. **StardustXR as the shell layer**: package via `buildRustPackage` + its own
   `nix/stardust-xr-server.nix` as reference; define a `stardust-session` systemd user
   target: `monado.socket` → `stardust-xr-server` → startup clients (config file at
   `~/.config/stardust/startup`), with `STARDUST_INSTANCE` propagation as designed.
8. **Atomic runtime closure updates**: Monado's undefined IPC ABI between service and
   client .so makes Nix-style whole-closure switching the *correct* update model — lean
   into it.

## 10. What Mura should reject and why

- **Envision's build-at-runtime orchestration** (git clone of branch heads into `~/.local
  /share`, `LD_LIBRARY_PATH` prefixes, mutable per-user stacks): unreproducible,
  unauditable, and redundant when the distro owns the build (vronlinux: "highly
  recommended to not use Envision" on NixOS).
- **Runtime mutation of `active_runtime.json`/`openvrpaths.vrpath`** (WiVRn's
  backup-and-symlink, Envision's `.envision.bak` dance): racy, breaks under sandboxes and
  read-only $HOME; on an appliance there is exactly one runtime — declare it in
  `/etc/xdg`. (Keep WiVRn's mechanism only if WiVRn coexists with another runtime.)
- **`steamvr_lh` / SteamVR-dependent tracking** as a default: pulls a proprietary Steam
  install into the trust base; only meaningful on desktop PCVR. Ship libsurvive/vive
  drivers instead; leave steamvr_lh behind an off-by-default option.
- **In-process runtime builds** (`XRT_FEATURE_SERVICE=OFF`) for the system runtime:
  loses socket activation, multi-client compositing, and the service/session split.
- **OpenVR/OpenComposite plumbing as a core module**: it exists for Steam game compat
  (`OVR_COMPAT_SEARCH_PATH`, openvrpaths); on a standalone headset distro it's optional
  at best — don't let its config contaminate the base option surface.
- **Branch-following upstreams anywhere in the build** (Envision defaults, Stardust's
  `[patch.crates-io]` branches): everything must resolve to rev+hash at eval time.
- **Relying on the vendor-Android + APK model** (WiVRn's client side) as the long-term
  headset story — it's the thing Mura exists to replace; but keep WiVRn server
  support as a bridge feature since its host packaging is excellent.

## 11. Open questions

1. **Display path on target hardware**: does `comp_window_vk_display` (VK_KHR_display)
   work on the target SoC's Vulkan driver, or is a minimal DRM-lease-capable Wayland
   compositor needed under Monado (`comp_window_direct_wayland.c`)? Who owns the DRM
   master in an XR-first session — Monado or a host compositor?
2. **2D app story under StardustXR**: with no in-tree Wayland compositor in the current
   server, which component provides `FLAT_WAYLAND_DISPLAY` for 2D apps in a headset-only
   session (Flatland? a headless host compositor?) — needs a decision and packaging.
3. **Driver strategy for the specific headset**: is there an existing Monado driver
   (in-tree list in `CMakeLists.txt`) for the target device, or does Mura maintain
   a patch series — and if so, what's the rebase cadence against Monado main (WiVRn
   demonstrates ~11 patches is sustainable)?
4. **SLAM/VIT plugin**: Basalt is the only production `libbasalt.so` provider
   (mateosss fork per Envision); is the VIT ABI (`t_vit_loader.c`) stable enough to pin
   Basalt and Monado independently, or must they be co-pinned like WiVRn/Monado?
5. **Hand-tracking model licensing/redistribution**: can the LFS-hosted ONNX models be
   baked into the image, or must they remain a separate fetched artifact?
6. **aarch64 status**: nixpkgs-xr/monado flakes declare `aarch64-linux`; what actually
   builds and runs (ONNXRuntime, Basalt, video encode) on the target ARM SoC?
7. **Multi-runtime coexistence**: if WiVRn (bridge) and native Monado both ship, how is
   runtime selection surfaced (`active_runtime.<arch>.json` per loader spec vs
   `XR_RUNTIME_JSON` per-session) without runtime symlink games?
8. **Upstreaming path**: which Mura patches (device driver, session integration)
   are candidates for Monado upstream to shrink the fork surface, given Monado's
   explicit "upstream as much as possible" guidance (`doc/writing-driver.md`)?
