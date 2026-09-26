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
use std::time::Duration;

use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::signals::{Signal, Signals};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{channel, EventLoop, Interest, Mode as CMode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
use smithay::desktop::PopupManager;
use smithay::utils::{Logical, Point};

use render::PlaneDraw;
use scene::M_PER_PX;
use state::{now_ns, spawn_client, TexRef, Zxr};
use xr::math;
use xr::{FrameTick, XrCore};

const USAGE: &str = "zxr [--socket NAME] [--control PATH] [--spawn CMD]... [--frames N] [--journal PATH] [--drm-node PATH] [--xwayland DISPLAY]\n       zxr ctl SOCKET COMMAND...";

struct Args {
    socket: Option<String>,
    control: Option<String>,
    spawn: Vec<String>,
    frames: Option<u64>,
    journal: Option<std::path::PathBuf>,
    drm_node: Option<String>,
    xwayland: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { socket: None, control: None, spawn: Vec::new(), frames: None, journal: None, drm_node: None, xwayland: None };
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
    let loader = std::env::var("ZXR_OPENXR_LOADER").ok().or_else(|| option_env!("MURA_OPENXR_LOADER").map(String::from)).unwrap_or_else(|| "libopenxr_loader.so.1".into());

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
                tracing::info!(display = %disp, "xwayland-satellite spawned");
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
    for c in &mut st.children {
        let _ = c.kill();
        let _ = c.wait();
    }
    let _ = std::fs::remove_file(&control_path);
    res.map_err(|e| e.to_string())
}

/// One entry in this frame's draw list: a surface placed on a plane.
struct Draw {
    model: math::Mat4,
    half_size: [f32; 2],
    tex: TexRef,
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
    let gpu_ns = st.renderer.read_gpu_time();
    st.release_held(slot, tick.frame_id);

    // 3. the draw list: every mapped plane's surface tree (+ popups), textures brought current
    let mut draws: Vec<Draw> = Vec::new();
    let plane_count = st.scene.planes.len();
    for pi in 0..plane_count {
        let (window, model, geo) = {
            let p = &st.scene.planes[pi];
            if p.mapped_at_frame == 0 {
                continue;
            }
            (p.window.clone(), p.model(), p.window.geometry())
        };
        let Some(root) = window.toplevel().map(|t| t.wl_surface().clone()) else { continue };
        let mut surfaces: Vec<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<i32, Logical>, smithay::utils::Size<i32, Logical>)> = Vec::new();
        collect_tree(&root, (0, 0).into(), &mut surfaces);
        for (popup, offset) in PopupManager::popups_for_surface(&root) {
            let loc = geo.loc + offset - popup.geometry().loc;
            collect_tree(popup.wl_surface(), loc, &mut surfaces);
        }
        let centre = [geo.loc.x as f32 + geo.size.w as f32 * 0.5, geo.loc.y as f32 + geo.size.h as f32 * 0.5];
        for (i, (surface, loc, size)) in surfaces.into_iter().enumerate() {
            let Some(tex) = st.update_surface_texture(&surface, tick.frame_id) else { continue };
            let cx = (loc.x as f32 + size.w as f32 * 0.5 - centre[0]) * M_PER_PX;
            let cy = -(loc.y as f32 + size.h as f32 * 0.5 - centre[1]) * M_PER_PX;
            // subsurfaces/popups sit a hair in front of their parent so the depth test orders them
            let local = math::model([cx, cy, i as f32 * 0.0005], 0.0);
            draws.push(Draw { model: math::mul(&model, &local), half_size: [size.w as f32 * M_PER_PX * 0.5, size.h as f32 * M_PER_PX * 0.5], tex });
        }
    }

    // 4. render both views into the acquired swapchain images
    let indices = st.xr.acquire_images()?;
    let flip = math::flip_y();
    let view_proj: Vec<math::Mat4> = views.iter().map(|v| math::mul(&math::mul(&flip, &math::projection(v.fov, 0.05, 100.0)), &math::view(v.pose))).collect();
    {
        let Zxr { renderer, dmabuf_textures, surface_tex, journal, .. } = st;
        let mut planes: Vec<PlaneDraw<'_>> = Vec::with_capacity(draws.len());
        let mut transitions = Vec::new();
        for d in &draws {
            let tex = match &d.tex {
                TexRef::Dmabuf(bid) => dmabuf_textures.get(bid).map(|(t, _)| t),
                TexRef::Surface(sid) => surface_tex.get(sid).and_then(|e| e.texture.as_ref()),
            };
            let Some(tex) = tex else {
                journal.stale_texture_draws += 1;
                continue;
            };
            if tex.dmabuf {
                transitions.push(tex.image);
            }
            planes.push(PlaneDraw { model: d.model, half_size: d.half_size, texture: tex, flip_v: false });
        }
        renderer.render(slot, &indices, &view_proj, &mut planes, [0.05, 0.05, 0.08, 1.0], &transitions)?;
    }
    st.xr.release_images()?;
    st.xr.end_frame(time, Some(&views))?;

    // 5. frame callbacks: once per refresh, after xrEndFrame (§6.6)
    let now = Duration::from_millis(st.now_ms() as u64);
    let output = st.output.clone();
    for p in &st.scene.planes {
        if p.mapped_at_frame != 0 {
            p.window.send_frame(&output, now, Some(Duration::ZERO), |_, _| Some(output.clone()));
            st.journal.frame_callbacks += 1;
        }
    }

    let wake_to_end = now_ns().saturating_sub(tick.woke_at_ns);
    let period = tick.predicted_display_period.as_nanos().max(1) as u64;
    st.journal.record_frame(true, gpu_ns, wake_to_end, wake_to_end > period);
    st.journal.fences_outstanding = st.renderer.frames.iter().filter(|f| f.in_use).count() as u64;
    finish_frame(st)
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
