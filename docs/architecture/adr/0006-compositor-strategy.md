# ADR 0006: XR compositor strategy — revive the zxr lineage as `zxr-shell-v2`, Wayland-native, on Monado

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [08-wxrc](../../research/08-wxrc.md) (Motorcar→wxrc→wxrd lineage + code),
[09-wxrc-ecosystem-gap-2026](../../research/09-wxrc-ecosystem-gap-2026.md) (2026 patch archaeology),
[10-xr-wayland-protocol-comparison](../../research/10-xr-wayland-protocol-comparison.md) (five-model
comparison), [05-xr-userspace](../../research/05-xr-userspace.md) (Monado runtime; StardustXR's
missing in-tree compositor). Relates to [adr/0005](0005-flake-layout-and-outputs.md) (nixpkgs-xr as
input) and the open 2D-app question in [05-xr-userspace §11](../../research/05-xr-userspace.md).

## Context

spatial-os needs an XR compositor/shell: the component that puts multiple applications into one
shared 3D space on a headset, keeping unmodified 2D Wayland apps first-class (the Motorcar
philosophy, [08](../../research/08-wxrc.md) Part 1 §1.1). The user co-authored the original
`zxr_shell_unstable_v1` protocol for wxrc (2019, with Drew DeVault and Simon Ser) and wants to
continue that work; it is intended to be the backbone of the spatial-os compositor.

The research established five candidate architectures ([10](../../research/10-xr-wayland-protocol-comparison.md)
§2), spanning "no new protocol" to "leave Wayland entirely":

- **motorcar** (2014, QtWayland): the ancestor — view-dependent depth-composited 3D windows,
  cuboid/portal clipping, 6DoF input. Dead since 2015.
- **zxr / wxrc** (2019, wlroots+OpenXR): motorcar re-expressed for OpenXR — N views, typed
  pixel/depth buffers, `wl_buffer` transport — but with input, clipping, timing, and size
  negotiation dropped. wxrc froze 2021; it is a ~3,460-line prototype on wlroots 0.8, GLES2+EGL,
  Monado-only, with the zxr depth path defined but **not actually wired** ([08](../../research/08-wxrc.md) §2.1).
- **wxrd** (2023, Collabora): the documented descendant — but it **dropped zxr entirely** and became
  "2D windows in VR via xrdesktop/gxr," wlroots 0.15, prototype-quality
  ([08](../../research/08-wxrc.md) §2.3).
- **zwin** (2022): the opposite philosophy — a serialized GLES 3.2 command stream executed by the
  compositor (server-side rendering). Powerful but a GPU-process-weight compositor; dormant since
  2023/2024 ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.3).
- **StardustXR** (active 2026): leaves Wayland for a scene-graph IPC; 2D delegated to a client
  compositor (Flatland). The strongest *running* 3D platform, but non-Wayland
  ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.4).
- **WayVR** (most active, 2026): zero new protocol — an embedded smithay compositor textured into
  flat `XR_EXTX_overlay` quads. Proves the 2D tier needs no protocol, but is an overlay accessory,
  not a session ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.5).

Two research findings are decisive:

1. **The 2019 patch burden is mostly gone** ([09](../../research/09-wxrc-ecosystem-gap-2026.md)):
   DRM leasing (`wp_drm_lease_v1`), Monado's EGL binding + Wayland direct mode, wlroots' GLES2
   texture access, Xwayland lease bridging, and Sway integration all LANDED; the Mesa/Vulkan
   Wayland-lease WSI patches were SUPERSEDED by the ratified `VK_EXT_acquire_drm_display`. No
   seven-fork upstream effort remains.
2. **The real work is a compositor rewrite, not a port** ([08](../../research/08-wxrc.md) §2.5,
   [09](../../research/09-wxrc-ecosystem-gap-2026.md) §4): every wlroots API wxrc uses predates the
   0.11+ scene-graph/renderer/allocator/buffer rework and is gone in 0.19.

## Decision

**Continue the zxr lineage as `zxr-shell-v2`: a Wayland-native, client-renders /
compositor-composites XR shell protocol, implemented in a new compositor that is itself an OpenXR
client of Monado.** The compositor serves ordinary `xdg-shell` for 2D apps and `zxr-shell-v2` for
3D apps, compositing both into one depth-tested space.

This is **option 4** from the plan (a new protocol informed by zxr + zwin + motorcar), implemented
in a fresh compositor — explicitly **not** a port of the wxrc codebase (option 1), **not** a revival
of wxrd (option 2), and **not** adopting StardustXR's substrate (option 3). StardustXR and WayVR are
instead **packaged as optional alternative sessions** ([adr/0005](0005-flake-layout-and-outputs.md),
[05 §9.7](../../research/05-xr-userspace.md)).

### The protocol: `zxr-shell-v2`

Per [10 §4.4](../../research/10-xr-wayland-protocol-comparison.md), refill the zxr skeleton with the
flesh motorcar had and the mechanisms 2026 provides:

- **Keep from zxr:** N `zxr_view` globals (add the missing resolution/fov events); typed per-view
  pixel/depth composite buffers; the XR-surface role with a 2D-buffer-attach protocol error.
- **Restore from motorcar:** the **view/projection/model matrix split** (compositor owns
  view+projection per view; client owns per-surface model transform) replacing zxr's folded
  per-surface-view MVP; **cuboid/portal clipping modes**; **3D size negotiation**
  (configure/ack_configure with a serial, borrowing zwin-shell's idiom); **6DoF pointer input**
  (surface-local position + orientation).
- **Borrow from zwin:** a **`wl_pointer`-shaped ray input device** (enter/leave/motion/button/axis
  + frame) as a second seat capability — rays are what controllers and hand-pinch produce; keep
  hit-test geometry compositor-derived, not client-uploaded.
- **Borrow from StardustXR (mechanism only):** **explicit sync via `wp_linux_drm_syncobj_v1`** on
  both pixel and depth buffers, with **linux-dmabuf** as the primary transport — this retires
  Motorcar's depth-viewport-packing hack ([08 §1.5](../../research/08-wxrc.md)); the client exports
  its real depth attachment as a dmabuf, no Mesa fork ([09 §5](../../research/09-wxrc-ecosystem-gap-2026.md)).
- **Add:** first-class **frame timing** mapping `xrWaitFrame`/`XrFrameState.predictedDisplayTime`
  onto a per-surface XR frame event carrying the predicted-display-time view matrices, plus
  `wp_presentation`-style feedback; and **free 2D** — an `xdg_toplevel` appears as a quad with no XR
  interface at all (compositor policy places it, thesis §6.1.2.2).

The protocol lives in-tree initially (`zxr-shell-v2.xml`), with an explicit intent to propose it to
`wayland-protocols` staging once it stabilizes — the user's authorship and the fact that the
original was reserved in the OpenXR/registry process ([09 §2](../../research/09-wxrc-ecosystem-gap-2026.md))
make upstreaming a realistic goal, not a fork.

### The renderer: Vulkan, not GLES2+EGL

wxrc's GLES2 + `XR_MNDX_egl_enable` path ties the compositor to Monado and to a GL interop Vulkan
never needed ([08 §2.5](../../research/08-wxrc.md), [09 §2](../../research/09-wxrc-ecosystem-gap-2026.md)).
Target **Vulkan with `XR_KHR_vulkan_enable2`**: Khronos-ratified (not the provisional MNDX
extension), matches Monado's native Vulkan path and the rest of the spatial-os XR stack
(StardustXR's `dmatex` is Vulkan+dmabuf+syncobj), and provides the external-memory/modifier/
sync primitives the dmabuf depth path needs. GLES2+MNDX remains available only as a throwaway
bring-up shortcut, never the production target.

### The compositor base: to be settled by a spike (wlroots 0.19 vs smithay)

Because this is a rewrite, the base library is genuinely open:

- **wlroots 0.19 (C):** wxrc's lineage; [09](../../research/09-wxrc-ecosystem-gap-2026.md) §4 maps
  the exact 0.19 API surface (scene graph, renderer, allocator, output-state, xdg-shell lifecycle);
  mature DRM-lease and Vulkan-renderer support.
- **smithay (Rust):** WayVR's base, which already demonstrates the entire 2D tier end-to-end
  (xdg-shell, popups, dmabuf-with-feedback, Xwayland) on a headset via `XR_EXTX_overlay`
  ([10 §2.5](../../research/10-xr-wayland-protocol-comparison.md)); matches the Rust of the rest of
  the XR ecosystem (StardustXR, WayVR, nixpkgs-xr) and gives memory safety for a compositor parsing
  untrusted client buffers.

**Current leaning (documented, NOT ratified): Rust + smithay.** Since this is a rewrite rather than
a port, the wxrc-lineage argument for C/wlroots is weak, and Rust is preferred: memory safety for a
compositor parsing untrusted client buffers and cross-process dmabuf/fd handles, and alignment with
the rest of the XR ecosystem this project already depends on (StardustXR, WayVR, nixpkgs-xr are all
Rust; WayVR's smithay stack already proves the 2D tier end-to-end on a headset). The expectation is
**C FFI where it counts** — Monado/OpenXR loader, `libwayland`/protocol scanning where needed, and
any wlroots-only helper without a mature Rust equivalent — via the usual `-sys` bindings.

This is a *leaning to write down*, not a hard decision. It is **not ratified**: the sub-decision is
still deferred to the **D2 spike** (compile wxrc against modern wlroots to size the port) plus a
concrete smithay/WayVR evaluation, and will be ratified as a follow-up amendment to this ADR once
those exist. Open checks before ratifying: smithay's coverage of the pieces we need beyond the 2D
tier (Vulkan renderer integration, DRM leasing, the OpenXR/`ash` boundary, explicit-sync via
`wp_linux_drm_syncobj_v1`), and whether any wlroots-only capability forces a larger C surface than
"FFI where it counts" implies. The protocol and the client-rendered / Monado-client / Vulkan
decisions above are base-independent (a wire protocol is language-agnostic), so none of this blocks
protocol work.

### Sequencing: ship the 2D tier first

Per [10 §4.4](../../research/10-xr-wayland-protocol-comparison.md) and WayVR's evidence, the
2D-panels-in-XR tier needs no new protocol and is independently useful. Ship it first (xdg-shell
quads composited by Monado, on the chosen base), then layer the `zxr-shell-v2` 3D-native tier onto
the same compositor. This de-risks the project: a usable 2D XR workspace exists before the novel 3D
protocol is finished, and it directly closes the open 2D-app question in
[05 §11](../../research/05-xr-userspace.md).

## Consequences

- A new module surface is added: `spatial.xr.shell` selecting the compositor/session
  (`zxr` = the spatial-os compositor | `stardust` | `wayvr` | `none`), and a compositor backend
  option. [overview.md](../overview.md) and `modules/xr/` are updated to reflect that the shell/2D
  path is now a defined layer, not an open question.
- The reusable assets from the lineage are the **zxr protocol design** and the **thesis philosophy**
  ([08](../../research/08-wxrc.md) Part 1) — not the wxrc or wxrd code, which are design references.
- The engineering program is the four items from [09](../../research/09-wxrc-ecosystem-gap-2026.md)'s
  bottom line: (1) the compositor on modern wlroots/smithay, (2) the Vulkan renderer, (3) the
  `zxr-shell-v2` depth/timing/input protocol, (4) hardware qualification across AMD/Intel/NVIDIA and
  real HMDs — sequenced behind the D1/D2 spikes and the display-path feasibility test that
  [overview.md](../overview.md) already names as the first XR experiment.
- `wp_drm_lease_v1` is consumed by Monado on the desktop/dev profile and bypassed by `VK_KHR_display`
  on the appliance ([10 §1](../../research/10-xr-wayland-protocol-comparison.md)); the new protocol
  needs nothing from it.
- Open protocol questions are carried in [10 §4.5](../../research/10-xr-wayland-protocol-comparison.md)
  (frame pacing across clients, 2D-toplevel→3D mapping, whether geometry ever crosses the wire, depth
  trust/clipping, and the depth-dmabuf driver matrix).
- The concrete composition model (renderer-agnostic opaque colour+depth "sort-last" baseline, the
  transparency/reprojection/light-transport tiers, the renderer-agnostic GL/Vulkan/CPU transport,
  first-class 2D windows, and the MVP milestones) is worked out in
  [zxr-shell-v2-composition.md](../zxr-shell-v2-composition.md), which also records the verification
  of the 2026 OpenXR spatial-container / depth-test-layer claims that bear on this decision.

## Alternatives considered

- **Port wxrc as-is (option 1):** rejected — it is a rewrite regardless
  ([08 §2.5](../../research/08-wxrc.md)); the code is a design reference, not a base.
- **Revive wxrd (option 2):** rejected — it dropped the zxr 3D-windowing protocol, routes through a
  heavy xrdesktop/gxr stack, and is a stale 2023 prototype ([08 §2.3](../../research/08-wxrc.md)).
- **zwin server-side rendering:** rejected — GPU-process-weight compositor, capped client rendering,
  dormant ecosystem ([10 §4.2](../../research/10-xr-wayland-protocol-comparison.md)).
- **StardustXR substrate:** rejected as the *backbone* — non-Wayland contradicts the Part 1
  philosophy and delegates 2D to a second compositor; kept as a packaged optional session.
- **WayVR only:** rejected as the *backbone* — an overlay with no shared 3D space or 3D apps; but
  adopted as the model for the first (2D) tier and packaged as-is.
