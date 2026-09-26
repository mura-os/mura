//! zxr — Mura's XR compositor, R0 bring-up (specs/zxr-core.md).
//!
//! One state loop (calloop) owns every Wayland object and the OpenXR frame begin/end; a
//! dedicated thread blocks in `xrWaitFrame` and posts frame ticks (ADR 0006 amendment
//! 2026-09-26). Each tick runs the frame procedure of spec §7 once.

mod control;
mod journal;
mod render;
mod scene;
mod state;
mod xr;

use std::os::unix::net::UnixListener;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use smithay::backend::renderer::utils::{with_renderer_surface_state, CommitCounter, RendererSurfaceStateUserData};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::signals::{Signal, Signals};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{channel, EventLoop, Interest, Mode as CMode, PostAction};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
use smithay::desktop::PopupManager;
use smithay::utils::{Logical, Point, Rectangle, Size};

use render::PlaneDraw;
use scene::M_PER_PX;
use state::{now_ns, spawn_client, DebugPanels, PanelSwapchain, TexRef, Zxr};
use xr::math;
use xr::{FrameTick, QuadLayer, XrCore};

const USAGE: &str = "zxr [--socket NAME] [--control PATH] [--spawn CMD]... [--frames N] [--journal PATH] [--drm-node PATH] [--xwayland DISPLAY] [--debug-panels projection]\n       zxr ctl SOCKET COMMAND...";

struct Args {
    socket: Option<String>,
    control: Option<String>,
    spawn: Vec<String>,
    frames: Option<u64>,
    journal: Option<std::path::PathBuf>,
    drm_node: Option<String>,
    xwayland: Option<String>,
    debug_panels: DebugPanels,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { socket: None, control: None, spawn: Vec::new(), frames: None, journal: None, drm_node: None, xwayland: None, debug_panels: DebugPanels::Auto };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--socket" => a.socket = Some(val()?),
            "--control" => a.control = Some(val()?),
            "--spawn" => a.spawn.push(val()?),
            "--frames" => a.frames = Some(val()?.parse().map_err(|e| format!("--frames: {e}"))?),
            "--journal" => a.journal = Some(val()?.into()),
            "--drm-node" => a.drm_node = Some(val()?),
            "--xwayland" => a.xwayland = Some(val()?),
            "--debug-panels" => {
                a.debug_panels = match val()?.as_str() {
                    "auto" => DebugPanels::Auto,
                    "projection" => DebugPanels::Projection,
                    other => return Err(format!("--debug-panels: {other} (auto|projection)")),
                }
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(a)
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("ctl") {
        if let Err(e) = control::client_main(&argv[2..]) {
            eprintln!("zxr ctl: {e}");
            std::process::exit(1);
        }
        return;
    }
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())).init();
    if let Err(e) = run() {
        eprintln!("zxr: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    // Block the signals the loop handles *before* any thread exists (the wait thread, Mesa's
    // workers inherit the mask): otherwise the kernel may deliver SIGTERM to a thread without
    // the signalfd and the default action kills the process before the journal is written.
    // SAFETY: plain sigset manipulation on the main thread at startup.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
    let loader = std::env::var("ZXR_OPENXR_LOADER").ok().or_else(|| option_env!("MURA_OPENXR_LOADER").map(String::from)).unwrap_or_else(|| "libopenxr_loader.so.1".into());

    // Scheduling (research/65 §4.3): the compositors converge on the *minimum* real-time
    // priority with RESET_ON_FORK so spawned clients never inherit it (KWin `realtime.cpp:17-26`,
    // gamescope `Process.cpp:619-636`); the runtime takes the maximum for itself. The standard
    // mechanism is the unit (`CPUSchedulingPolicy=rr`, `CPUSchedulingPriority=1`,
    // `CPUSchedulingResetOnFork=yes`); this is the fallback when zxr runs outside its unit, and
    // it fails quietly without CAP_SYS_NICE or an RLIMIT_RTPRIO grant. Done before any thread
    // exists so the wait thread inherits it.
    request_realtime();

    let (mut xr, vk) = XrCore::new(&loader, "zxr")?;
    let extents: Vec<_> = xr.swapchains.iter().map(|s| s.extent).collect();
    let images: Vec<_> = xr.swapchains.iter().map(|s| s.images.clone()).collect();
    let renderer = render::Renderer::new(&vk, xr.color_format, &extents, &images)?;
    let ticks = xr.take_ticks().unwrap();

    let mut event_loop: EventLoop<'static, Zxr> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let display: Display<Zxr> = Display::new().map_err(|e| e.to_string())?;
    let mut st = Zxr::new(display, event_loop.handle(), event_loop.get_signal(), xr, renderer, args.socket.as_deref(), args.drm_node.as_deref())?;
    st.frames_limit = args.frames;
    st.journal_path = args.journal.clone();
    st.debug_panels = args.debug_panels;
    tracing::info!(debug_panels = ?st.debug_panels, "composition: quads always, projection only with depth content (ADR 0006 amendment 2)");
    tracing::info!(socket = ?st.socket_name, "listening");

    // frame ticks from the wait thread
    event_loop
        .handle()
        .insert_source(ticks, |ev, _, state| {
            if let channel::Event::Msg(tick) = ev {
                if let Err(e) = on_tick(state, tick) {
                    tracing::error!("frame: {e}");
                    state.loop_signal.stop();
                }
            }
        })
        .map_err(|e| e.to_string())?;

    // runtime events independent of frame ticks: session READY/STOPPING/EXITING arrive before
    // any frame can (xrWaitFrame is only legal on a running session), so poll them on a timer
    // while not running; once running, every tick polls too and this becomes a slow check.
    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_millis(5)), |_, _, state| {
            if let Err(e) = state.xr.poll_events() {
                tracing::error!("runtime events: {e}");
                state.loop_signal.stop();
                return TimeoutAction::Drop;
            }
            if state.xr.exit_requested {
                state.loop_signal.stop();
                return TimeoutAction::Drop;
            }
            TimeoutAction::ToDuration(if state.xr.session_running { Duration::from_millis(250) } else { Duration::from_millis(5) })
        })
        .map_err(|e| e.to_string())?;

    // signals: SIGUSR1 dumps the journal, SIGINT/SIGTERM quit
    event_loop
        .handle()
        .insert_source(Signals::new(&[Signal::SIGUSR1, Signal::SIGINT, Signal::SIGTERM]).map_err(|e| e.to_string())?, |ev, _, state| match ev.signal() {
            Signal::SIGUSR1 => print!("{}", state.journal.render(now_ns())),
            _ => state.loop_signal.stop(),
        })
        .map_err(|e| e.to_string())?;

    // control socket
    let control_path = args.control.clone().unwrap_or_else(|| format!("{}/zxr-{}.sock", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()), std::process::id()));
    let _ = std::fs::remove_file(&control_path);
    let listener = UnixListener::bind(&control_path).map_err(|e| format!("control socket {control_path}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    tracing::info!(path = %control_path, "control socket");
    event_loop
        .handle()
        .insert_source(Generic::new(listener, Interest::READ, CMode::Level), |_, listener, state| {
            while let Ok((stream, _)) = listener.accept() {
                let _ = stream.set_nonblocking(false);
                control::serve_line(stream, |cmd| handle_control(state, cmd));
            }
            Ok(PostAction::Continue)
        })
        .map_err(|e| e.to_string())?;

    // Xwayland via the satellite (gate 4): an ordinary client with an X display of its own
    if let Some(disp) = &args.xwayland {
        let bin = option_env!("MURA_XWAYLAND_SATELLITE").unwrap_or("xwayland-satellite");
        match spawn_client(&format!("exec {bin} {disp}"), &st.socket_name, None) {
            Ok(c) => {
                tracing::info!(display = %disp, pid = c.id(), "xwayland-satellite spawned");
                st.satellite_pid = Some(c.id());
                st.children.push(c);
            }
            Err(e) => tracing::warn!("xwayland-satellite: {e}"),
        }
    }
    for cmd in &args.spawn {
        match spawn_client(cmd, &st.socket_name, args.xwayland.as_deref()) {
            Ok(c) => st.children.push(c),
            Err(e) => tracing::warn!(cmd, "spawn: {e}"),
        }
    }

    // the loop: ticks, clients, control; flush after every dispatch
    let res = event_loop.run(None, &mut st, |state| {
        let _ = state.dh.flush_clients();
    });
    let journal = st.journal.render(now_ns());
    print!("{journal}");
    if let Some(p) = &st.journal_path {
        let _ = std::fs::write(p, &journal);
    }
    // orderly exit: session out of the running state first, then `Drop for Zxr` orders the rest
    st.xr.shutdown();
    drop(st);
    let _ = std::fs::remove_file(&control_path);
    res.map_err(|e| e.to_string())
}

/// One entry in this frame's draw list: a surface placed on a plane.
/// One mapped plane's surface tree for this tick (spec §7 rev 3).
struct PlaneTree {
    root_id: smithay::reexports::wayland_server::backend::ObjectId,
    model: math::Mat4,
    yaw: f32,
    geo: Rectangle<i32, Logical>,
    /// (texture, location relative to the toplevel origin, logical size), tree order
    items: Vec<(TexRef, Point<i32, Logical>, Size<i32, Logical>)>,
    /// hash of every surface's commit count, location and size — the panel is dirty when it changes
    signature: u64,
    /// geometry ∪ every surface rect, relative to the toplevel origin
    bounds: Rectangle<i32, Logical>,
    dist2: f32,
}

/// Spec §7, once per tick.
fn on_tick(st: &mut Zxr, tick: FrameTick) -> Result<(), String> {
    st.xr.poll_events()?;
    if st.xr.exit_requested {
        st.loop_signal.stop();
        return Ok(());
    }
    if !st.xr.session_running {
        return Ok(());
    }
    st.frame_id = tick.frame_id;
    st.xr.begin_frame(&tick)?;
    let time = tick.predicted_display_time;

    if !tick.should_render {
        st.xr.end_frame(time, None)?;
        st.journal.record_frame(false, None, now_ns().saturating_sub(tick.woke_at_ns), false);
        return finish_frame(st);
    }

    // 1. views for the predicted display time; the head pose drives the gaze pointer
    let views = st.xr.locate_views(time)?;
    let head = views[0].pose;
    st.update_gaze_pointer(head);

    // 2. this frame's slot: wait for its previous submission, release what it sampled
    let slot = (tick.frame_id % 2) as usize;
    st.slot = slot;
    st.renderer.wait_slot(slot)?;
    let gpu_ns = if st.journal.last_tick_submitted { st.renderer.read_gpu_time() } else { None };
    st.release_held(slot, tick.frame_id);

    // 3. every mapped plane's surface tree (toplevel + subsurfaces + popups), textures brought
    //    current, held for this slot; the tree's commit signature and its bounds decide whether
    //    the plane's panel needs a pass (spec §6.2 rev 3: one pass per commit, never per frame).
    let head_pos = [head.position.x, head.position.y, head.position.z];
    let mut trees: Vec<PlaneTree> = Vec::new();
    for pi in 0..st.scene.planes.len() {
        let (window, model, pos, yaw, geo) = {
            let p = &st.scene.planes[pi];
            if p.mapped_at_frame == 0 {
                continue;
            }
            (p.window.clone(), p.model(), p.pos, p.yaw, p.window.geometry())
        };
        let Some(root) = window.toplevel().map(|t| t.wl_surface().clone()) else { continue };
        let mut surfaces: Vec<(WlSurface, Point<i32, Logical>, Size<i32, Logical>)> = Vec::new();
        collect_tree(&root, (0, 0).into(), &mut surfaces);
        for (popup, offset) in PopupManager::popups_for_surface(&root) {
            let loc = geo.loc + offset - popup.geometry().loc;
            collect_tree(popup.wl_surface(), loc, &mut surfaces);
        }
        let mut hasher = DefaultHasher::new();
        let mut bounds = geo;
        let mut items = Vec::with_capacity(surfaces.len());
        for (surface, loc, size) in surfaces {
            let Some(tex) = st.update_surface_texture(&surface, tick.frame_id) else { continue };
            let count = with_renderer_surface_state(&surface, |s| s.current_commit().distance(Some(CommitCounter::default())).unwrap_or(0)).unwrap_or(0);
            (surface.id().protocol_id(), count, loc.x, loc.y, size.w, size.h).hash(&mut hasher);
            bounds = bounds.merge(Rectangle::new(loc, size));
            items.push((tex, loc, size));
        }
        let d = [pos[0] - head_pos[0], pos[1] - head_pos[1], pos[2] - head_pos[2]];
        trees.push(PlaneTree { root_id: root.id(), model, yaw, geo, items, signature: hasher.finish(), bounds, dist2: d[0] * d[0] + d[1] * d[1] + d[2] * d[2] });
    }

    // 4. the quad budget (spec §7 rev 3): the nearest planes are quad layers up to the runtime's
    //    cap minus one; the rest overflow into the projection layer, which then exists.
    let budget = if st.debug_panels == DebugPanels::Projection { 0 } else { st.quad_budget() };
    let dist2: Vec<f32> = trees.iter().map(|t| t.dist2).collect();
    let (quad_idx, overflow_idx) = scene::select_quads(&dist2, budget);
    let depth = st.depth_content_present(!overflow_idx.is_empty());

    // 4a. panel swapchains: create/resize to the tree bounds; a pass only when the signature changed
    let mut dirty: Vec<usize> = Vec::new();
    for &ti in &quad_idx {
        let t = &trees[ti];
        let (w, h) = (t.bounds.size.w.max(1) as u32, t.bounds.size.h.max(1) as u32);
        let need_new = st.panel_swapchains.get(&t.root_id).map(|ps| ps.sc.extent.width != w || ps.sc.extent.height != h).unwrap_or(true);
        if need_new {
            if let Some(old) = st.panel_swapchains.remove(&t.root_id) {
                st.renderer.destroy_target(old.target);
            }
            let sc = st.xr.create_panel_swapchain(w, h)?;
            let target = st.renderer.make_panel_target(sc.extent, st.xr.color_format, &sc.images)?;
            st.panel_swapchains.insert(t.root_id.clone(), PanelSwapchain { sc, target, signature: None, bounds: t.bounds, passes: 0 });
        }
        let ps = st.panel_swapchains.get_mut(&t.root_id).unwrap();
        ps.bounds = t.bounds;
        if ps.signature != Some(t.signature) {
            dirty.push(ti);
        }
    }
    {
        let live: std::collections::HashSet<_> = quad_idx.iter().map(|&i| trees[i].root_id.clone()).collect();
        let Zxr { panel_swapchains, renderer, .. } = &mut *st;
        let gone: Vec<_> = panel_swapchains.keys().filter(|k| !live.contains(*k)).cloned().collect();
        for k in gone {
            if let Some(ps) = panel_swapchains.remove(&k) {
                renderer.destroy_target(ps.target);
            }
        }
    }

    // 4b. GPU work — only if there is any: dirty panel passes and/or the projection pass
    let submit = !dirty.is_empty() || depth;
    if submit {
        let Zxr { renderer, dmabuf_textures, surface_tex, journal, panel_swapchains, xr, .. } = &mut *st;
        let resolve = |r: &TexRef| -> Option<&render::Texture> {
            match r {
                TexRef::Dmabuf(bid) => dmabuf_textures.get(bid).map(|(t, _)| t),
                TexRef::Surface(sid) => surface_tex.get(sid).and_then(|e| e.texture.as_ref()),
            }
        };
        // foreign-queue barriers for every dmabuf sampled this tick (panels + overflow)
        let mut foreign: Vec<ash::vk::Image> = Vec::new();
        for &ti in dirty.iter().chain(overflow_idx.iter()) {
            for (tex, _, _) in &trees[ti].items {
                if let Some(t) = resolve(tex) {
                    if t.dmabuf && !foreign.contains(&t.image) {
                        foreign.push(t.image);
                    }
                }
            }
        }
        let cmd = renderer.begin_frame(slot, &foreign)?;
        // panel passes: orthographic, pixels → NDC (y down, as the framebuffer), textures flipped
        for &ti in &dirty {
            let t = &trees[ti];
            let ps = panel_swapchains.get_mut(&t.root_id).unwrap();
            let idx = xr.acquire_panel_image(&mut ps.sc)?;
            let (w, h) = (ps.sc.extent.width as f32, ps.sc.extent.height as f32);
            let ortho = math::ortho_px(w, h);
            let mut planes: Vec<PlaneDraw<'_>> = Vec::with_capacity(t.items.len());
            for (i, (tex, loc, size)) in t.items.iter().enumerate() {
                let Some(texture) = resolve(tex) else {
                    journal.stale_texture_draws += 1;
                    continue;
                };
                let cx = (loc.x - t.bounds.loc.x) as f32 + size.w as f32 * 0.5;
                let cy = (loc.y - t.bounds.loc.y) as f32 + size.h as f32 * 0.5;
                planes.push(PlaneDraw { model: math::model([cx, cy, -(i as f32) * 0.001], 0.0), half_size: [size.w as f32 * 0.5, size.h as f32 * 0.5], texture, flip_v: true });
            }
            renderer.record_pass(cmd, &ps.target, idx, &ortho, &planes, [0.0, 0.0, 0.0, 0.0]);
            ps.signature = Some(t.signature);
            ps.passes += 1;
            renderer.panel_passes += 1;
            renderer.panel_bytes += (w * h) as u64 * 4 * 2;
        }
        // the projection pass: overflow planes (and, from M2, volumes / environment / cutout)
        if depth {
            let projection_indices = xr.acquire_images()?;
            let flip = math::flip_y();
            for (vi, v) in views.iter().enumerate() {
                let view_proj = math::mul(&math::mul(&flip, &math::projection(v.fov, 0.05, 100.0)), &math::view(v.pose));
                let mut planes: Vec<PlaneDraw<'_>> = Vec::new();
                for &ti in &overflow_idx {
                    let t = &trees[ti];
                    let centre = [t.geo.loc.x as f32 + t.geo.size.w as f32 * 0.5, t.geo.loc.y as f32 + t.geo.size.h as f32 * 0.5];
                    for (i, (tex, loc, size)) in t.items.iter().enumerate() {
                        let Some(texture) = resolve(tex) else {
                            journal.stale_texture_draws += 1;
                            continue;
                        };
                        let cx = (loc.x as f32 + size.w as f32 * 0.5 - centre[0]) * M_PER_PX;
                        let cy = -(loc.y as f32 + size.h as f32 * 0.5 - centre[1]) * M_PER_PX;
                        let local = math::model([cx, cy, i as f32 * 0.0005], 0.0);
                        planes.push(PlaneDraw { model: math::mul(&t.model, &local), half_size: [size.w as f32 * M_PER_PX * 0.5, size.h as f32 * M_PER_PX * 0.5], texture, flip_v: false });
                    }
                }
                let clear = if quad_idx.is_empty() { [0.05, 0.05, 0.08, 1.0] } else { [0.0, 0.0, 0.0, 0.0] };
                renderer.record_pass(cmd, renderer.view_target(vi), projection_indices[vi], &view_proj, &planes, clear);
            }
        }
        renderer.end_frame(slot, &foreign)?;
        // releases only after the work is queued (the runtime waits on the queue, not the CPU)
        for &ti in &dirty {
            let ps = panel_swapchains.get_mut(&trees[ti].root_id).unwrap();
            xr.release_panel_image(&mut ps.sc)?;
        }
        if depth {
            xr.release_images()?;
        }
    }
    st.journal.last_tick_submitted = submit;
    if depth {
        st.journal.projection_layer_frames += 1;
    } else {
        st.journal.panels_only_frames += 1;
    }

    // 4c. xrEndFrame: the projection layer (if any) first, then the quads nearest-last within
    //     the band (painter's order, `rendering.adoc:1143-1147`)
    {
        let Zxr { xr, panel_swapchains, .. } = &mut *st;
        let mut quads: Vec<QuadLayer<'_>> = Vec::with_capacity(quad_idx.len());
        for &ti in quad_idx.iter().rev() {
            let t = &trees[ti];
            let Some(ps) = panel_swapchains.get(&t.root_id) else { continue };
            if ps.signature.is_none() {
                continue;
            }
            // the quad is centred on the panel bounds, offset from the plane's geometry centre
            let dx = ((ps.bounds.loc.x as f32 + ps.bounds.size.w as f32 * 0.5) - (t.geo.loc.x as f32 + t.geo.size.w as f32 * 0.5)) * M_PER_PX;
            let dy = -((ps.bounds.loc.y as f32 + ps.bounds.size.h as f32 * 0.5) - (t.geo.loc.y as f32 + t.geo.size.h as f32 * 0.5)) * M_PER_PX;
            let p = math::transform_point(&t.model, [dx, dy, 0.0]);
            let (s, c) = (t.yaw * 0.5).sin_cos();
            quads.push(QuadLayer {
                swapchain: &ps.sc,
                pose: openxr::Posef { orientation: openxr::Quaternionf { x: 0.0, y: s, z: 0.0, w: c }, position: openxr::Vector3f { x: p[0], y: p[1], z: p[2] } },
                size: [ps.bounds.size.w as f32 * M_PER_PX, ps.bounds.size.h as f32 * M_PER_PX],
            });
        }
        xr.end_frame_with_quads(time, if depth { Some(&views) } else { None }, &quads)?;
    }

    // 5. frame callbacks: once per refresh, after xrEndFrame (§6.6)
    let now = Duration::from_millis(st.now_ms() as u64);
    let output = st.output.clone();
    // Visibility-gated (research/65 §4.2; niri `niri.rs:5178-5208`, KWin `item.cpp:739-751`,
    // mutter `meta-wayland.c:182-219` converge): a plane in either view's frustum gets its callback
    // every tick; a plane out of view gets one on a fallback cadence (niri: 995 ms) so a client
    // waiting on a callback never stalls, but stops driving the GPU at display rate.
    const FALLBACK_TICKS: u64 = 60;
    let journal = &mut st.journal;
    for p in &mut st.scene.planes {
        if p.mapped_at_frame != 0 {
            let visible = views.iter().any(|v| plane_in_view(v, &p.model(), p.half_size()));
            let due = tick.frame_id.saturating_sub(p.last_frame_callback) >= FALLBACK_TICKS;
            if visible {
                journal.frame_callbacks_visible += 1;
            } else {
                journal.frame_callbacks_occluded += 1;
            }
            if visible || due {
                p.window.send_frame(&output, now, Some(Duration::ZERO), |_, _| Some(output.clone()));
                p.last_frame_callback = tick.frame_id;
                journal.frame_callbacks += 1;
            }
        }
    }

    let wake_to_end = now_ns().saturating_sub(tick.woke_at_ns);
    let period = tick.predicted_display_period.as_nanos().max(1) as u64;
    st.journal.record_frame(true, gpu_ns, wake_to_end, wake_to_end > period);
    st.journal.fences_outstanding = st.renderer.frames.iter().filter(|f| f.in_use).count() as u64;
    st.xr.calls.wait_frame.add(tick.wait_ns);
    st.journal.calls = st.xr.calls.clone();
    st.journal.passes_per_frame = st.renderer.passes_per_frame();
    st.journal.attachment_bytes_est = if depth { st.renderer.attachment_bytes_per_frame() } else { 0 };
    st.journal.panel_passes = st.renderer.panel_passes;
    st.journal.panel_bytes = st.renderer.panel_bytes;
    st.journal.panel_swapchains = st.panel_swapchains.len() as u64;
    finish_frame(st)
}

fn request_realtime() {
    // SAFETY: plain sched_setscheduler on the calling thread.
    unsafe {
        let min = libc::sched_get_priority_min(libc::SCHED_RR);
        let param = libc::sched_param { sched_priority: min };
        if libc::sched_setscheduler(0, libc::SCHED_RR | libc::SCHED_RESET_ON_FORK, &param) == 0 {
            tracing::info!(policy = "SCHED_RR", priority = min, "real-time scheduling granted");
        } else {
            tracing::info!("real-time scheduling not granted (no CAP_SYS_NICE / RLIMIT_RTPRIO); running SCHED_OTHER");
        }
    }
}

/// Frustum test: any of the plane's four corners in front of the view and within its fov.
fn plane_in_view(view: &openxr::View, model: &math::Mat4, half: [f32; 2]) -> bool {
    let v = math::view(view.pose);
    let tl = view.fov.angle_left.tan();
    let tr = view.fov.angle_right.tan();
    let td = view.fov.angle_down.tan();
    let tu = view.fov.angle_up.tan();
    let corners = [[-half[0], -half[1], 0.0], [half[0], -half[1], 0.0], [-half[0], half[1], 0.0], [half[0], half[1], 0.0]];
    corners.iter().any(|c| {
        let w = math::transform_point(model, *c);
        let e = math::transform_point(&v, w);
        if e[2] >= -0.01 {
            return false;
        }
        let x = e[0] / -e[2];
        let y = e[1] / -e[2];
        x >= tl && x <= tr && y >= td && y <= tu
    })
}

fn finish_frame(st: &mut Zxr) -> Result<(), String> {
    if st.frame_id % 60 == 0 {
        st.gc_textures(st.frame_id);
    }
    if let Some(limit) = st.frames_limit {
        if st.journal.frames >= limit {
            tracing::info!(frames = st.journal.frames, "frame limit reached");
            st.loop_signal.stop();
        }
    }
    Ok(())
}

/// Every surface in the tree with its location (relative to the root's origin) and logical size.
fn collect_tree(root: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, location: Point<i32, Logical>, out: &mut Vec<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<i32, Logical>, smithay::utils::Size<i32, Logical>)>) {
    with_surface_tree_downward(
        root,
        location,
        |_, states, loc: &Point<i32, Logical>| {
            let view = states.data_map.get::<RendererSurfaceStateUserData>().and_then(|d| d.lock().unwrap().view());
            match view {
                Some(v) => TraversalAction::DoChildren(*loc + v.offset),
                None => TraversalAction::SkipChildren,
            }
        },
        |surface, states, loc: &Point<i32, Logical>| {
            let view = states.data_map.get::<RendererSurfaceStateUserData>().and_then(|d| d.lock().unwrap().view());
            if let Some(v) = view {
                out.push((surface.clone(), *loc + v.offset, v.dst));
            }
        },
        |_, _, _| true,
    );
}

fn handle_control(st: &mut Zxr, cmd: control::Command) -> String {
    use control::Command::*;
    match cmd {
        FocusNext => {
            st.scene.focus_next();
            let f = st.scene.focused;
            st.focus_window(f);
            format!("focused {:?}", f)
        }
        List => {
            let mut s = String::new();
            for (i, p) in st.scene.planes.iter().enumerate() {
                let title = p.window.toplevel().map(|t| smithay::wayland::compositor::with_states(t.wl_surface(), |s| s.data_map.get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>().unwrap().lock().unwrap().title.clone().unwrap_or_default())).unwrap_or_default();
                let g = p.window.geometry();
                s.push_str(&format!("{i}{} {}x{} pos=({:.2},{:.2},{:.2}) yaw={:.2} mapped={} {title}\n", if Some(i) == st.scene.focused { "*" } else { " " }, g.size.w, g.size.h, p.pos[0], p.pos[1], p.pos[2], p.yaw, p.mapped_at_frame != 0));
            }
            s.trim_end().to_string()
        }
        Journal => st.journal.render(now_ns()).trim_end().to_string(),
        Quit => {
            st.loop_signal.stop();
            "bye".into()
        }
        Close => match st.scene.focused_window().and_then(|w| w.toplevel().cloned()) {
            Some(t) => {
                t.send_close();
                "closed".into()
            }
            None => "no focus".into(),
        },
        Resize(w, h) => match st.scene.focused_window().and_then(|w| w.toplevel().cloned()) {
            Some(t) => {
                t.with_pending_state(|s| s.size = Some((w, h).into()));
                t.send_pending_configure();
                format!("configure {w}x{h}")
            }
            None => "no focus".into(),
        },
        Move(dx, dy, dz) => match st.scene.focused.and_then(|i| st.scene.planes.get_mut(i)) {
            Some(p) => {
                p.pos[0] += dx;
                p.pos[1] += dy;
                p.pos[2] += dz;
                format!("pos=({:.2},{:.2},{:.2})", p.pos[0], p.pos[1], p.pos[2])
            }
            None => "no focus".into(),
        },
        Key(code, state) => {
            match state {
                Some(pressed) => st.send_key(code, pressed),
                None => {
                    st.send_key(code, true);
                    st.send_key(code, false);
                }
            }
            format!("key {code}")
        }
        Type(text) => {
            let mut n = 0;
            for c in text.chars() {
                if let Some(code) = control::ascii_keycode(c) {
                    st.send_key(code, true);
                    st.send_key(code, false);
                    n += 1;
                }
            }
            format!("typed {n}")
        }
        Spawn(cmd) => match spawn_client(&cmd, &st.socket_name, None) {
            Ok(c) => {
                let pid = c.id();
                st.children.push(c);
                format!("pid {pid}")
            }
            Err(e) => format!("error {e}"),
        },
        Unknown(l) => format!("unknown: {l}"),
    }
}
