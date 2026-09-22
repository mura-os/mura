# 09 — wxrc ecosystem gap analysis, 2026

**Date:** 2026-09-22  
**Scope:** the external patch stack named by wxrc: DRM leasing, OpenXR, Monado, wlroots, Mesa, Vulkan, Xwayland, and Sway.  
**Question:** which 2019 patches landed, which were superseded, and what still has to be built for spatial-os?

## Executive finding

wxrc was a small C Wayland compositor which put ordinary 2D surfaces and experimental depth-bearing 3D surfaces into an OpenXR scene. It was based on wlroots and initially used EGL/OpenGL ES to submit that scene to Monado. Its README calls it “reasonably usable” for 2D content and proof-of-concept quality for 3D content, while warning that it needed patches across the graphics stack ([wxrc repository](https://git.sr.ht/~sircmpwn/wxrc)).

Most of that intimidating patch stack is no longer an upstream-patching problem. DRM leasing, Monado's Wayland direct mode, Monado's EGL graphics binding, wlroots' GLES2 texture-access API, Sway's lease integration, and Xwayland's lease bridge all landed. The proposed Wayland-specific Vulkan and Mesa WSI patches did not land in their original form; they were replaced by the cleaner, ratified, file-descriptor-based `VK_EXT_acquire_drm_display`.

The remaining work is concentrated in wxrc itself:

- a substantial port from 2019-era wlroots to the 0.19 API family;
- a renderer decision: retain the Monado-specific EGL path for quick bring-up, or move to Vulkan for the production design;
- a new, synchronized color-plus-depth transport;
- modern frame timing, explicit synchronization, and input protocol work.

The crucial negative result is that generic Wayland DMA-BUF did **not** make native depth/stencil sharing portable. The wxrc-specific EGL Registry discussion says cross-vendor non-color DMA-BUF sharing still needed more design and recommends Vulkan external-memory/GL interop instead ([Khronos EGL issue 133](https://github.com/KhronosGroup/EGL-Registry/issues/133)). “Depth via DMA-BUF” is possible only after choosing an explicit representation and synchronization contract; it is not a solved consequence of using `zwp_linux_dmabuf_v1`.

### Verdict vocabulary

- **LANDED** — the required capability exists upstream in the 2026 stack.
- **SUPERSEDED** — the original patch/API did not land, but a different upstream mechanism supplies the capability.
- **STILL-MISSING** — spatial-os still needs new implementation or design work; this does not necessarily mean an upstream fork is required.

Where no recoverable patch set identifies an exact change, this document marks the conclusion as **inference** rather than silently inventing patch history.

---

## 1. DRM leasing — enabling infrastructure

### What the 2019 patch was for

An HMD commonly appears as a DRM connector marked `non-desktop`. The desktop compositor is DRM master, but an XR runtime needs exclusive, low-latency control of that connector. DRM leasing lets the compositor grant a restricted DRM master file descriptor covering selected connector/CRTC/plane resources to the runtime.

Drew DeVault's contemporary account gives the original sequence: define a Wayland lease protocol, implement it in wlroots and Sway, add Vulkan WSI and Mesa support, then bridge it through Xwayland ([“DRM leasing: VR for Wayland”](https://drewdevault.com/blog/DRM-leasing-and-VR-for-Wayland/)). The protocol discussion identifies VR HMDs as the primary use case and explains why `non-desktop` connectors should be handed to clients rather than added to the desktop layout ([wayland-devel v7 patch](https://lore.freedesktop.org/wayland-devel/BY3XLC2LXH2K.15NMIQNWQK9DG@homura/t/)).

This was not a mechanism for drawing Wayland windows in wxrc. It was the lower-level route by which Monado or SteamVR could drive the headset while another Wayland compositor owned the card.

### What exists in 2026

`wp_drm_lease_v1` remains a **staging** protocol at `staging/drm-lease/drm-lease-v1.xml`. Staging encourages implementation and deployment, but an incompatible revision would require a new major version ([wayland-protocols policy](https://github.com/wayland-mirror/wayland-protocols)). The protocol advertises connectors, accepts a connector request, and returns a lease FD or a rejection/revocation event ([protocol reference](https://wayland.emersion.fr/protocol/drm-lease-v1.html)).

The requested compositor milestones are verified:

- **Sway 1.7** added “support for virtual reality headsets via DRM leasing” ([Sway 1.7 release](https://github.com/swaywm/sway/releases/tag/1.7)).
- **KDE Plasma 5.24** advertised Wayland VR-headset support with optimal performance ([Plasma 5.24 announcement](https://kde.org/announcements/plasma/5/5.24.0/)).
- **Mutter 47** added DRM-lease protocol support ([Mutter NEWS](https://github.com/GNOME/mutter/blob/86097755798e96b10ae167086acbd0eaf2688804/NEWS), [MR !3746](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/3746)).

Monado's current `comp_window_direct_wayland` target binds the lease global, tracks connector IDs, receives `lease_fd`, and creates the direct-mode target ([source reference](https://monado.pages.freedesktop.org/monado/comp__window__direct__wayland_8c.html)). Monado's direct-mode documentation says its “wayland direct” target uses `drm-lease-v1` to acquire an HMD ([Monado direct mode](https://monado.freedesktop.org/direct-mode.html)).

wlroots 0.19 exposes a complete lease-manager API: offer an output, grant or reject requests, and revoke leases ([wlroots DRM-lease API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_drm_lease_v1.h.html)). Use 0.19.1 or newer rather than 0.19.0, whose lease path had a reported use-after-free ([labwc fix reference](https://github.com/labwc/labwc/pull/2887)).

### 2026 status: **LANDED**

No spatial-os fork of wayland-protocols, wlroots, a host compositor, or Monado is required for basic HMD leasing. The NixOS image must select compatible versions and test `non-desktop` recognition, hotplug, revocation, multi-GPU selection, and session restart.

---

## 2. OpenXR EGL binding — `XR_MNDX_egl_enable`

### What the 2019 patch was for

Standard OpenXR had GLX/Xlib and platform-specific OpenGL bindings, but wxrc was an EGL-based Wayland compositor. It needed to pass its existing `EGLDisplay`, `EGLConfig`, and `EGLContext` to the runtime when creating an `XrSession`.

Simon Ser reserved the original `XR_MND_egl_enable` extension in 2019 ([OpenXR-Docs PR 39](https://github.com/KhronosGroup/OpenXR-Docs/pull/39)). The full provisional specification credited Simon Ser and Drew DeVault's registry work, then adopted the multi-vendor `MNDX` prefix ([OpenXR-Docs PR 48](https://github.com/KhronosGroup/OpenXR-Docs/pull/48)). wxrc's compatibility commit visibly changes `XrGraphicsBindingEGLMND` and `XR_MND_*` to `MNDX` forms ([wxrc rename commit](https://git.sr.ht/~sircmpwn/wxrc/commit/b7457446a0542298ee01f519975685383951b55b)).

### What exists in 2026

The extension is registered as number 49, revision 2, requiring OpenXR 1.0. Its registry page reports **Ratification Status: Not ratified** ([OpenXR manual page](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_MNDX_egl_enable.html)), and it remains in the OpenXR 1.1 provisional list ([OpenXR 1.1.47](https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html)). That is a portability warning, not evidence that the implementation was removed.

`XrGraphicsBindingEGLMNDX` carries `getProcAddress`, `display`, `config`, and `context`; clients compile with `XR_USE_PLATFORM_EGL`, enable the extension, and place the binding in `XrSessionCreateInfo::next` ([binding reference](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrGraphicsBindingEGLMNDX.html)).

Monado still lists `XR_MNDX_egl_enable` as its desktop alternative to `XR_KHR_opengl_enable` and `XR_KHR_opengl_es_enable` ([Monado extensions](https://monado.freedesktop.org/)). Its current path validates an OpenGL or OpenGL ES context and creates an EGL-backed client compositor ([Monado EGL implementation](https://github.com/DisplayXR/displayxr-runtime/blob/main/src/xrt/state_trackers/oxr/oxr_session_gfx_egl.c)). Modern Godot Wayland work reports that the nominal `XrGraphicsBindingOpenGLWaylandKHR` is insufficient in practice and uses `XR_MNDX_egl_enable` ([Godot PR 97771](https://github.com/godotengine/godot/pull/97771)).

### GL or Vulkan in 2026?

For a **Monado-only resurrection**, GL-on-OpenXR remains viable. It is the shortest route to a first frame because wxrc already uses MNDX and wlroots still publishes `wlr_gles2_texture_get_attribs` ([wlroots GLES2 API](https://wlroots.pages.freedesktop.org/wlroots/wlr/render/gles2.h.html)).

For a **distribution architecture**, Vulkan is safer:

- `XR_KHR_vulkan_enable2` is Khronos-defined rather than provisional MNDX ([OpenXR Vulkan API](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrGetVulkanGraphicsDevice2KHR.html)).
- Monado already requires Vulkan external-memory and external-semaphore support for its native image path ([Monado getting started](https://monado.freedesktop.org/getting-started.html)).
- Vulkan exposes explicit external memory, DRM modifiers, synchronization, and layout transitions needed by the depth design.
- wlroots has had an opt-in Vulkan renderer since 0.15, built on `VK_EXT_image_drm_format_modifier` and `VK_EXT_physical_device_drm` ([initial renderer commit](https://git.nixnet.services/blankie/wlroots/commit/8e346922508aa3eaccd6e12f2917f6574f349843)).

Vulkan does not magically solve depth transport across Wayland. It supplies the primitives from which spatial-os can define that transport.

### 2026 status: **LANDED**

The exact Monado EGL capability wxrc needed is upstream. Keep it as a bring-up path, but treat a Vulkan renderer as the production redesign unless spatial-os deliberately accepts Monado/EGL coupling.

---

## 3. Monado

### What the 2019 patches were for

No single archived “wxrc Monado patch set” is recoverable from the public search surface. Primary evidence supports three work streams:

1. **EGL/OpenGL ES OpenXR sessions.** Simon Ser's Monado list includes merged “Add GLES support” MR !156 in October 2019 ([Monado MRs by emersion](https://gitlab.freedesktop.org/monado/monado/-/merge_requests?scope=all&utf8=%E2%9C%93&state=all&author_username=emersion)). Current source still credits Drew DeVault and Simon Ser on EGL session code ([file reference](https://monado.pages.freedesktop.org/monado/oxr__session__gfx__egl_8c.html)).
2. **A modern Wayland/backend foundation.** Drew's October 2019 update says Monado merged a build-system overhaul, an overhaul of its dated Wayland backend, and OpenXR-conformance work ([October 2019 update](https://drewdevault.com/blog/Status-update-October-2019/)).
3. **Wayland DRM-lease direct mode.** MR !683 explicitly says it continued Drew DeVault's closed MR !141, updating it for the new `comp_target` API, revised lease protocol, and Vulkan headers ([Monado MR !683](https://gitlab.freedesktop.org/monado/monado/-/merge_requests/683)).

The November 2019 announcement says wxrc moved from proprietary SteamVR APIs to OpenXR because of these Monado contributions ([November 2019 update](https://drewdevault.com/blog/Status-update-November-2019/)).

### What exists in 2026

Monado describes itself as an OpenXR-conformant runtime supporting Vulkan, OpenGL, OpenGL ES, and MNDX EGL ([Monado project](https://monado.freedesktop.org/)). Its tree retains the EGL client-compositor path ([client compositor docs](https://monado.pages.freedesktop.org/monado/group__comp__client.html)) and `comp_window_direct_wayland` ([source reference](https://monado.pages.freedesktop.org/monado/comp__window__direct__wayland_8c.html)).

The historically identifiable changes are upstream:

- EGL/OpenGL ES graphics binding: upstream.
- MNDX extension handling: upstream.
- Wayland window/backend modernization: upstream and since refactored.
- Wayland DRM-lease direct mode: upstream.
- OpenXR conformance fixes: incorporated into modern Monado.

### 2026 status: **LANDED**

Use stock Monado. spatial-os needs runtime configuration, hardware-driver selection, reproducible device tests, and failure handling, but no recovered 2019 Monado patch. The uncertainty is historical attribution at commit granularity, not capability.

---

## 4. wlroots — the main source-port gap

### What the 2019 patches were for

Two original wlroots needs are verifiable.

First, DRM leasing lived in wlroots, while Sway chose which outputs to offer. The original PR was superseded by a reworked implementation ([PR 1730](https://github.com/swaywm/wlroots/pull/1730), [PR 2929](https://github.com/swaywm/wlroots/pull/2929)).

Second, wxrc needed the GL object behind a `wlr_texture` so its custom shader could place a client surface in a 3D scene. PR 1901 says compositors previously needed an intermediate off-screen render, adds `wlr_gles2_texture_get_attribs`, and links wxrc as its example ([wlroots PR 1901](https://github.com/swaywm/wlroots/pull/1901)). It merged in November 2019 and remains in 0.19.

Later wxrc work drove a custom `wl_resource`-to-`wlr_buffer` path; its proposal says it rewrote earlier wxrc-used work, but that PR was closed for a later revision ([wlroots PR 2934](https://github.com/swaywm/wlroots/pull/2934)). The exact downstream set used by each wxrc revision is not recoverable, so anything beyond leasing and texture access is inference.

### Baseline

wlroots 0.7 shipped in August 2019 ([0.7 release](https://github.com/swaywm/wlroots/releases/tag/0.7.0)). wxrc does not publicly pin a version, so “0.7–0.10 era” is safer than inventing an exact ABI. wlroots has no stable ABI and versions its shared library by minor release ([compatibility explanation](https://docs.rs/crate/wlr-sys/0.19.0)).

The requested target is the maintained 0.19 line. Version 0.19.3 includes Vulkan synchronization fixes relevant to foreign textures ([0.19.3 changelog](https://github.com/NetBSD/pkgsrc/commit/a9e9484b0ad4d63448c95cf3e901a74c37ccc297)).

### Break class A — scene graph and damage ownership

`wlr_scene` did not exist in wxrc's original architecture. It is now wlroots' declarative tree for 2D surfaces, buffers, rectangles, per-output damage, frame scheduling, direct scanout, and protocol helpers ([scene API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_scene.h.html)).

It intentionally supports basic 2D composition only; complex effects require custom rendering ([scene header](https://github.com/external-mirrors/wlroots/blob/2cec06a4/include/wlr/types/wlr_scene.h)). For wxrc:

- use `wlr_scene` for xdg-shell/Xwayland lifetime, subsurfaces, stacking, output-enter/leave, and damage;
- keep an XR render pass mapping selected buffers onto world-space quads or depth-composited views;
- bridge scene damage/frame needs into the OpenXR frame loop.

By 0.19, output integration centers on `wlr_scene_output_build_state`, `wlr_scene_output_commit`, and `wlr_scene_output_needs_frame` ([scene API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_scene.h.html)). This is an architectural rewrite, not symbol renaming.

### Break class B — renderer abstraction

The old pattern made an output's EGL context current and issued GL calls directly ([historical tutorial](https://drewdevault.com/blog/Writing-a-Wayland-compositor-1/)). Later releases removed public GLES2 internals, replaced texture rendering with subtexture rendering, and ultimately removed the old renderer interface ([0.11](https://github.com/swaywm/wlroots/releases/tag/0.11.0), [0.12](https://github.com/swaywm/wlroots/releases/tag/0.12.0), [0.18](https://newreleases.io/project/freedesktop-gitlab/wlroots/wlroots/release/0.18.0)).

The 0.13 transition removed two assumptions important to wxrc-like renderers: wlroots' framebuffer was no longer EGL's default framebuffer and no longer had a depth attachment ([renderer-v6 PR](https://github.com/swaywm/wlroots/pull/2240)).

Current choices:

- GLES2: use `wlr_gles2_texture_get_attribs` and own wxrc's XR framebuffer/depth attachment.
- Vulkan: use wlroots' Vulkan renderer but design a safe custom path for sampling buffers in the OpenXR graph.
- Renderer-neutral: import `wlr_buffer` DMA-BUF attributes into a separate XR renderer, accepting more interop/lifetime work.

### Break class C — allocator, buffer, and swapchain ownership

wlroots 0.13 introduced `wlr_allocator`, `wlr_swapchain`, renderer binding to buffers, and explicit producer/consumer ownership ([0.13 release](https://github.com/swaywm/wlroots/releases/tag/0.13.0)). Consumers lock/unlock; producers drop; allocators negotiate format/modifier sets ([buffer redesign](https://github.com/swaywm/wlroots/pull/2044), [allocator API](https://wlroots-38153e.pages.freedesktop.org/wlr/render/allocator.h.html)).

0.15 required compositors to create renderer and allocator explicitly and initialize each output ([0.15 changes](https://github.com/swaywm/wlroots/issues/2983)). wxrc code which stores raw `wl_buffer`, borrows textures across frames, or assumes backend-owned EGL resources must be rewritten around these lifetimes.

### Break class D — output state, layers, and backend lifecycle

Direct `wlr_output_set_*`, pending fields, attach/rollback rendering, and ad-hoc commits became `wlr_output_state`, test/commit, and render passes. 0.18 removed the old rendering interface ([0.18 notes](https://newreleases.io/project/freedesktop-gitlab/wlroots/wlroots/release/0.18.0)).

Output layers use `wlr_output_layer` plus `wlr_output_state_set_layers`; rejected candidates fall back to composition ([output-layers example](https://github.com/external-mirrors/wlroots/blob/2cec06a4/examples/output-layers.c)). Backend creation, output-layout creation, globals, scheduling, damage coordinates, listener cleanup, and commit events also changed.

wxrc's OpenXR swapchain is not a KMS output. The port should avoid faking a normal `wlr_output` and treat the XR loop as a distinct sink consuming wlroots-managed client buffers.

### Break class E — xdg-shell and Xwayland object models

wlroots removed `xdg-shell-unstable-v6` for stable `wlr_xdg_shell` ([0.12 release](https://github.com/swaywm/wlroots/releases/tag/0.12.0)). In 0.18, `new_surface` began firing immediately, toplevel/popup creation got role-specific events, destroy timing changed, and compositors became responsible for scheduling initial configure after `initial_commit` ([0.18 notes](https://newreleases.io/project/freedesktop-gitlab/wlroots/wlroots/release/0.18.0)).

Modern Xwayland surfaces have associate/dissociate phases before their `wlr_surface` is valid ([Xwayland API](https://wlroots.pages.freedesktop.org/wlroots/wlr/xwayland/xwayland.h.html)). Replace the old wxrc view wrapper rather than patching through these differences.

### 2026 status: **STILL-MISSING**

The specific upstream patches—DRM leasing and public GLES2 texture attributes—landed. The wxrc-to-wlroots-0.19 port does not exist and is the largest bring-up item. Estimate it as a compositor-core rewrite preserving policy and protocol ideas, not a compatibility patch.

---

## 5. Mesa

### What the recoverable 2019 Mesa patch was for

The strongest primary evidence does **not** tie the 2019 Mesa patch to Wayland EGL depth export. It ties it to DRM leasing.

Drew's sequence names “an implementation for Mesa's Vulkan WSI implementation” after proposing `VK_EXT_acquire_wl_display` ([DRM-leasing article](https://drewdevault.com/blog/DRM-leasing-and-VR-for-Wayland/)). The Vulkan proposal links Mesa MR !1509 ([Vulkan PR 1001](https://github.com/KhronosGroup/Vulkan-Docs/pull/1001)). Mesa MR !8981 says it continues closed !1509, rebased to the revised lease protocol ([Mesa MR !8981](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/8981)).

Therefore:

- **Verified:** the recovered Mesa patch was Vulkan WSI integration for the Wayland lease proposal.
- **Not recovered:** a 2019 Mesa patch exposing a Wayland EGL client's depth attachment.
- **Inference:** Motorcar correctly anticipated a Mesa change for native depth access, but wxrc's README is not evidence that such a patch existed.

### Why the original Mesa patch is unnecessary

The Wayland-specific Vulkan proposal was replaced by `VK_EXT_acquire_drm_display`, which takes a DRM FD and connector ID rather than Wayland protocol objects. It is ratified and provides `vkGetDrmDisplayEXT` and `vkAcquireDrmDisplayEXT` ([Vulkan reference](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_acquire_drm_display.html)). Monado receives the lease FD from Wayland, then uses generic DRM display acquisition. Mesa needs no Wayland-lease WSI fork.

### Does modern DMA-BUF solve depth?

No, not generically.

`EGL_EXT_image_dma_buf_import` imports an image described by DRM FourCC, plane FD, offset, and pitch ([EGL import](https://registry.khronos.org/EGL/extensions/EXT/EGL%5FEXT%5Fimage%5Fdma%5Fbuf%5Fimport.txt)). Its modifier extension negotiates tiling/compression and formats ([modifier extension](https://registry.khronos.org/EGL/extensions/EXT/EGL%5FEXT%5Fimage%5Fdma%5Fbuf%5Fimport%5Fmodifiers.txt)). Neither defines portable depth/stencil semantics.

The discussion opened for wxrc records the blocker: depth FourCCs and a depth/stencil EGL-image binding were absent, aliasing RGBA8 to D24S8 is invalid, and cross-vendor non-color sharing needed more design ([EGL issue 133](https://github.com/KhronosGroup/EGL-Registry/issues/133)). Mesa historically rejects export for Z16/S8 images lacking DRM FourCCs ([Mesa discussion](https://lists.freedesktop.org/archives/mesa-dev/2018-July/200479.html)).

Modern explicit sync solves a different problem. `wp_linux_drm_syncobj_v1` supplies per-surface acquire/release timeline points for DMA-BUFs ([protocol](https://wayland.app/protocols/linux-drm-syncobj-v1)). wlroots gained it in 0.18 ([release](https://newreleases.io/project/freedesktop-gitlab/wlroots/wlroots/release/0.18.0)); Mesa 24.1 added explicit sync to all Vulkan drivers on Wayland/X11 ([Mesa announcement](https://lists.freedesktop.org/archives/mesa-dev/2024-May/226222.html)). Synchronization does not supply a depth format.

Vulkan provides stronger blocks: `VK_EXT_external_memory_dma_buf` imports/exports DMA-BUF-backed memory, while `VK_EXT_image_drm_format_modifier` carries explicit image layout ([external memory](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_external_memory_dma_buf.html), [modifier](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_image_drm_format_modifier.html)). The wxrc EGL discussion recommends Vulkan external-memory/GL interop with explicit barriers, not raw EGL depth DMA-BUF ([EGL issue 133](https://github.com/KhronosGroup/EGL-Registry/issues/133)).

### 2026 status: **SUPERSEDED** for the recovered patch; depth remains **STILL-MISSING**

Do not carry the old Mesa WSI patch or budget a Mesa fork as the default depth plan. Define one of:

1. depth encoded in a negotiated color-sample buffer and reconstructed by the compositor;
2. a Vulkan external-memory image with explicit format, modifier, layout, ownership, and timeline semantics;
3. a same-process/client-library path for an initial prototype.

Option 1 is most portable. Option 2 offers highest performance but the largest driver matrix.

---

## 6. Vulkan

### What the 2019 patch was for

It was not a validation tweak or generic external-memory fix. It was `VK_EXT_acquire_wl_display`, a Wayland analogue of `VK_EXT_acquire_xlib_display`, allowing Vulkan direct-display code to obtain an HMD through Wayland DRM leasing ([original RFC](https://github.com/KhronosGroup/Vulkan-Docs/pull/1001)). It had corresponding Vulkan-Headers/Loader branches and Mesa WSI work.

### What replaced it

The refreshed proposal closed in June 2021 and says it was superseded by the FD-based design ([Vulkan PR 1450](https://github.com/KhronosGroup/Vulkan-Docs/pull/1450)). `VK_EXT_acquire_drm_display` generalized the operation:

- Wayland obtains and transfers the DRM lease FD.
- Vulkan maps a DRM connector ID to `VkDisplayKHR`.
- Vulkan acquires that display with the leased FD.

The registered extension is ratified, revision 1, and depends on `VK_EXT_direct_mode_display` ([extension reference](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_acquire_drm_display.html)).

This direct-display extension is separate from wxrc's renderer API. An EGL/OpenGL wxrc can run atop Monado while Monado uses Vulkan internally to drive the HMD.

### 2026 status: **SUPERSEDED**

Use released Vulkan headers and loader. No custom standard or loader patch is required. For a Vulkan wxrc renderer use `XR_KHR_vulkan_enable2`; for HMD direct mode let Monado own leasing and display acquisition.

---

## 7. Xwayland

### What the 2019 patch was for

The recoverable patch was **not** primarily “turn an X window into a textured quad.” Rootless Xwayland already rendered X clients into Wayland surfaces which a compositor could texture.

The patch implemented `drm-lease-v1` in Xwayland so an X11 VR application using RandR/Vulkan direct display—especially SteamVR—could request the HMD lease through its Wayland host. Its commit says it lets X11 clients lease non-desktop connectors ([Xwayland commit](https://github.com/Jie1zhang/xorg-xserver/commit/089e7f98f86836fdd09fd231bff8004c0fc45381)). It exposed a non-desktop RandR output, populated modes, and set `CONNECTOR_ID` for Xlib WSI ([current source](https://github.com/external-mirrors/xorg-xserver/blob/197582d9/hw/xwayland/xwayland-drm-lease.c)).

### What exists in 2026

The implementation shipped in Xwayland 22.1 ([release report](https://en.ubunlog.com/xwayland-22-1-0-arrives-with-support-for-drm-lease-gesture-improvements-for-touchpads-and-more/)). wlroots 0.19 can supervise a lazy rootless Xwayland server/XWM ([wlroots API](https://wlroots.pages.freedesktop.org/wlroots/wlr/xwayland/xwayland.h.html)).

`xwayland-satellite` is a compelling alternative. It acts as a Wayland client and XWM, presenting rootless X apps to any compositor implementing `xdg_wm_base` and `wp_viewporter` ([README](https://github.com/Supreeeme/xwayland-satellite/blob/main/README.md)). Version 0.7 added `-listenfd` for on-demand integration ([release](https://github.com/Supreeeme/xwayland-satellite/releases/tag/v0.7)).

Thus X apps can arrive as ordinary xdg toplevels and be placed on textured quads without wxrc maintaining an XWM. That does not make an X app a zxr depth-aware 3D client; it remains a 2D surface in 3D.

### 2026 status: **LANDED**

No Xwayland patch is required. Prefer xwayland-satellite for the first port unless ICCCM/EWMH requirements force wlroots' XWM. The host still needs correct focus, popup, clipboard, drag-and-drop, and projected pointer coordinates.

---

## 8. Sway

### What the 2019 patch was for

wxrc did not need Sway as a library or parent compositor. It was a separate wlroots compositor; its README says it is based on OpenXR and wlroots ([wxrc repository](https://git.sr.ht/~sircmpwn/wxrc)).

The Sway patch belonged to DRM leasing. wlroots exposed `non-desktop`; Sway created the lease manager and offered those outputs. The original PR says it prevents Sway extending the desktop onto VR headsets and makes them leasable ([Sway PR 4289](https://github.com/swaywm/sway/pull/4289)). It was superseded by later `wlr_drm_lease_v1` integration ([Sway PR 6284](https://github.com/swaywm/sway/pull/6284)).

Sway was also the principal reference consumer for changing wlroots APIs. That is source precedent, not a runtime dependency.

### What exists in 2026

Sway 1.7 released VR-headset leasing and depended on wlroots 0.15 ([Sway 1.7](https://github.com/swaywm/sway/releases/tag/1.7)). Monado direct mode runs under Sway without the 2019 patch.

spatial-os may use a conventional host compositor during development: run wxrc nested/windowed for iteration, or run Monado against the host lease interface. The final shell does not need Sway merely because wxrc listed it.

### 2026 status: **LANDED**

Drop Sway from wxrc's required build closure unless intentionally used as a development host/session. Use current Sway source as a wlroots migration reference, not code to patch.

---

## Summary

| Component | 2019 patch purpose | 2026 status | What spatial-os must do |
|---|---|---|---|
| DRM leasing / wayland-protocols | Lease a non-desktop HMD to an XR runtime | **LANDED** | Package current users; test detection, hotplug, revocation, multi-GPU |
| OpenXR | Pass EGL display/config/context via `XR_MNDX_egl_enable` | **LANDED**, provisional | Keep for GL bring-up; prefer `XR_KHR_vulkan_enable2` long term |
| Monado | EGL/GLES sessions, conformance, modern Wayland, lease direct mode | **LANDED** | Use stock Monado; build hardware/runtime tests |
| wlroots | Lease manager and GLES2 texture access for custom 3D rendering | **STILL-MISSING** as a wxrc port | Rewrite around 0.19 scene, buffer, allocator, renderer, output, shell lifecycles |
| Mesa | Proposed Wayland-specific Vulkan lease WSI; no recovered depth patch | **SUPERSEDED**; generic depth still missing | Use normal Mesa; define depth above the driver |
| Vulkan | `VK_EXT_acquire_wl_display` | **SUPERSEDED** | Use `VK_EXT_acquire_drm_display`; let Monado handle direct mode |
| Xwayland | Forward DRM leases to X11 VR clients and expose RandR connector/modes | **LANDED** | Use modern rootless Xwayland or xwayland-satellite |
| Sway | Offer non-desktop wlroots outputs for leasing | **LANDED** | No production dependency; optionally use as host/reference |

---

## Bring up wxrc in 2026 — dependency checklist

### A. Needs zero downstream patches

- [ ] **wayland-protocols:** released staging `wp_drm_lease_v1`.
- [ ] **Host compositor:** Sway 1.7+, Plasma 5.24+, Mutter 47+, or another compositor advertising DRM leasing.
- [ ] **Monado:** upstream `comp_window_direct_wayland`; do not revive MR !141.
- [ ] **OpenXR headers/loader:** current headers with MNDX revision 2 and `XR_KHR_vulkan_enable2`.
- [ ] **Vulkan:** released headers/loader/driver with `VK_EXT_acquire_drm_display`, direct display, and Monado's external-memory/semaphore requirements.
- [ ] **Mesa:** normal current package; do not apply !1509/!8981.
- [ ] **wlroots DRM leasing:** 0.19.1+ if spatial-os itself offers leases.
- [ ] **Xwayland:** current version containing DRM-lease support.
- [ ] **Sway:** carry no patch.

### B. Needs a fresh, relatively small spatial-os patch series

- [ ] Update build definitions and probes for current Meson, OpenXR, Wayland generation, wlroots 0.19, and cglm.
- [ ] Remove old `XR_MND_*` assumptions and verify revision-2 `PFN_xrEglGetProcAddressMNDX`.
- [ ] Add deterministic Monado/runtime/HMD/direct-vs-windowed selection and diagnostics.
- [ ] Add NixOS modules, udev/session dependencies, package options, and a simulated smoke test.
- [ ] Integrate xwayland-satellite startup, `DISPLAY`, focus, and clipboard if chosen.
- [ ] Add capability negotiation and rejection paths for unsupported 3D buffer format, modifier, synchronization, or depth representation.

These should be independent patches and should not modify Mesa, Monado, Vulkan, Xwayland, or Sway.

### C. Needs a substantial wlroots 0.19 rewrite

- [ ] Create backend, renderer, and allocator explicitly.
- [ ] Replace old output calls with `wlr_output_state` and tested commits where physical outputs are used.
- [ ] Replace old begin/end drawing with current render passes or explicitly owned XR targets.
- [ ] Adopt current `wlr_buffer` locking and release rules.
- [ ] Stop retaining borrowed textures/buffers past their valid lifetime.
- [ ] Use `wlr_scene` for 2D topology and protocol bookkeeping.
- [ ] Add an XR adapter turning selected scene buffers into world-space quads without pretending `wlr_scene` is 3D.
- [ ] Rebuild xdg-shell around toplevel/popup events, `initial_commit`, configure scheduling, and destroy ordering.
- [ ] Rebuild Xwayland around associate/dissociate, or delete it for xwayland-satellite.
- [ ] Rework listener teardown for current wlroots lifetime assertions.
- [ ] Reconcile OpenXR predicted-display-time frames with wlroots commits, callbacks, damage, and releases.
- [ ] Add GPU reset, swapchain recreation, runtime loss, HMD unplug, and lease revocation recovery.

This is the dominant engineering item for ADR 0006. Estimate protocol/lifetime tests, not just a successful compile.

### D. Needs design, not patch archaeology

- [ ] **Choose the production renderer.**
  - Fastest milestone: GLES/EGL plus `XR_MNDX_egl_enable`.
  - Preferred architecture: Vulkan plus `XR_KHR_vulkan_enable2`.
- [ ] **Specify color-plus-depth buffers.**
  - Do not label an arbitrary color FourCC as a native depth format.
  - Portable baseline: encode linearized depth in a negotiated color-sample buffer and reconstruct it.
  - Advanced path: define Vulkan external-memory format, modifier, layout, queue ownership, and synchronization.
- [ ] **Specify synchronization.** `wp_linux_drm_syncobj_v1` is per-`wl_surface` and requires acquire/release points for its attached buffer ([protocol](https://wayland.app/protocols/linux-drm-syncobj-v1)). zxr composite buffers are out-of-band, so spatial-os must bind them to a surface commit or add equivalent per-buffer timeline semantics.
- [ ] **Specify frame timing.** Include predicted display time, view poses, submission deadline, missed-frame behavior, and whether one slow client may stall the compositor.
- [ ] **Specify atomic pairing.** Both eyes and both buffer types need one frame identity; independent `wl_buffer` arrival is insufficient.
- [ ] **Specify depth math.** Define projection convention, near/far mapping, reversed-Z policy, normalization, precision, invalid values, and clipping.
- [ ] **Specify 3D input.** Preserve Wayland seat semantics for 2D apps while adding rays, poses, focus, grabs, and haptics.
- [ ] **Specify trust boundaries.** Limit dimensions, formats, modifier counts, FDs, timeline growth, and GPU retention for untrusted clients.

### E. Recommended bring-up sequence

1. Build old wxrc semantics against current OpenXR/Monado in a nested, single-GPU session.
2. Render one xdg-shell client as a quad using GLES/EGL.
3. Complete the wlroots 0.19 lifecycle port before adding depth.
4. Add rootless X apps through xwayland-satellite and verify projected input.
5. Bring up Monado direct mode under Sway or KWin with a real HMD.
6. Define and prototype encoded-depth transport with explicit acquire/release synchronization.
7. Measure copies, GPU waits, missed frames, and motion-to-photon latency.
8. Prototype Vulkan in parallel; retire GL only after Wayland DMA-BUF, OpenXR swapchains, and depth all work.
9. Add multi-GPU after the same-GPU path has stable modifier negotiation and observable synchronization.

---

## Biggest risks and unknowns

### 1. Depth remains a protocol and portability problem

The 2021 wxrc-specific EGL discussion is strong evidence that native depth DMA-BUF was not portable. No 2026 source found here establishes a cross-vendor EGL/Wayland depth-attachment path. The encoded-depth fallback may still be the correct portable baseline.

### 2. Vulkan does not eliminate interop complexity

Vulkan makes ownership explicit, but the compositor still negotiates importable formats/modifiers and synchronizes every producer/consumer transition. Older GPUs may lack `VK_EXT_image_drm_format_modifier`; wlroots' Vulkan renderer depends on it ([wlroots renderer issue](https://github.com/swaywm/sway/issues/8977)).

### 3. `wlr_scene` is 2D

It is valuable for lifecycle and damage but cannot be the XR scene graph. The adapter boundary between `wlr_scene_buffer` lifetime and OpenXR rendering is the highest-risk compositor-core design point.

### 4. The old patch list mixed two products

Mesa, Vulkan, Sway, and Xwayland were largely the SteamVR/direct-mode Wayland stack. OpenXR EGL and Monado were the wxrc rendering stack. Treating all seven as direct wxrc build dependencies would inflate the estimate and revive obsolete patches.

### 5. Hardware behavior is less uniform than protocol support

A compositor can advertise leasing while connector classification, EDID quirks, permissions, direct-display support, or GPU selection fails. Mutter's original MR noted that a real headset and GPU hot-unplug were not tested ([Mutter MR !3746](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/3746)). ADR 0006 should budget AMD, Intel, and NVIDIA test systems.

### 6. Original patch recovery is incomplete

The lease, Vulkan, Mesa WSI, Sway, Xwayland, Monado direct-mode, and wlroots texture changes have traceable artifacts. A separate 2019 Mesa depth patch and definitive per-commit wxrc Monado series were not found. The planning assumption should be “do not depend on unrecovered patches,” not “recreate whatever the README implied.”

## Bottom line for ADR 0006

The 2019 ecosystem patch burden has mostly disappeared. Do not estimate seven upstream forks. Estimate:

1. one major wlroots/compositor port;
2. one renderer decision and likely GL-to-Vulkan migration;
3. one new synchronized 3D color/depth/timing/input protocol;
4. one hardware qualification program.

That is substantial, but bounded and mostly under spatial-os's control.

---

## Appendix — build spikes (D1, D2)

Two spikes were run to check this analysis against reality (spike sources:
[`pkgs/wxrc-archaeology/`](../../pkgs/wxrc-archaeology/default.nix) and
[`spikes/wxrc-modern-probe.nix`](../../spikes/wxrc-modern-probe.nix)).

### D1 — era-pinned archaeology build: NOT CONSTRUCTIBLE from stock nixpkgs

wxrc's `meson.build` requires wlroots `>=0.8.1,<0.9.0` **and** a Monado OpenXR runtime with
`XR_MNDX_egl_enable`. No nixpkgs channel ever shipped both simultaneously (versions evaluated
live from the pinned tarballs):

| nixpkgs channel | wlroots | openxr-loader | monado | satisfies wxrc? |
|---|---|---|---|---|
| nixos-19.09 | 0.7.0 | 1.0.2 | **absent** | no — wlroots even older than 0.8.1, no Monado |
| nixos-20.09 | 0.11.0 | 1.0.11 | **absent** | no — wlroots too new, no Monado |
| nixos-21.11 | 0.14.1 | 1.0.20 | 21.0.0 | no — wlroots far past `<0.9` |
| nixos-23.11 | 0.16.2 | 1.0.31 | 2023-08 | no — wlroots far past `<0.9` |
| current (flake) | 0.20.2 | 1.1.62 | 25.1.0 | no — see D2 |

The wlroots 0.8.x window (2019) predates Monado's arrival in nixpkgs (~21.05); by then wlroots is
≥0.14. So wxrc-as-2021 depended on a **patched, out-of-tree wlroots 0.8 plus a then-bleeding-edge
OpenXR/Monado stack at the same time** — precisely the "patch large swaths of the ecosystem" the
README warns of. This confirms *from the packaging side* that reviving wxrc is a rewrite, not a
port (ADR 0006). A genuine archaeology build would require vendoring a 2019 patched wlroots 0.8 tree
plus a pinned Monado source build carrying the then-unmerged `XR_MNDX_egl_enable` work — out of
scope; the negative result is the finding.

### D2 — modern-port probe: wxrc does not preprocess against wlroots 0.19.3

Building wxrc against `wlroots_0_19` (0.19.3) from the flake's nixpkgs, after relaxing only the
`wlroots` version constraint, fails immediately at the **first `#include`**:

- `fatal error: wlr/types/wlr_surface.h: No such file` — 8 of the 10 source files include it; it
  was folded into `wlr/types/wlr_compositor.h` in wlroots 0.16+. The build dies here, before most
  deeper breaks are even reached.
- `'wlr_backend_impl' has no member named 'get_renderer'` — the renderer-ownership rework (break
  class B): `wlr_backend_get_renderer` and the backend `get_renderer` hook are gone.
- `initialization ... from incompatible pointer type` on the backend/renderer wiring — the
  `wlr_backend_autocreate(display, create_renderer)` callback signature (break class B) no longer
  exists.
- `invalid use of undefined type` — the custom `wlr_buffer_impl` / `wlr_output` plumbing against
  now-opaque or restructured types (break classes C/D).

Per-file error counts: `backend.c` 17, the rest 1–3 each — i.e. the XR backend and buffer glue are
the most API-coupled. The probe stops at the missing-header wall, which is itself the headline
result: **wxrc's source does not survive contact with a modern wlroots header set**, empirically
confirming §4's "compositor-core rewrite, not a compatibility patch." The spike derivation captures
`meson.log`, `ninja.log`, and a classified `summary.txt` for reproducibility.
