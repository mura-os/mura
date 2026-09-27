//! zxr — Mura's XR compositor, R0 bring-up (specs/zxr-core.md).
//!
//! One state loop (calloop) owns every Wayland object and the OpenXR frame begin/end; a
//! dedicated thread blocks in `xrWaitFrame` and posts frame ticks (ADR 0006 amendment
//! 2026-09-26). Each tick runs the frame procedure of spec §7 once.

mod control;
mod input;
mod journal;
mod render;
mod scene;
mod settings;
mod state;
mod xr;

use std::os::unix::net::UnixListener;
use std::time::Duration;

use smithay::backend::renderer::utils::{with_renderer_surface_state, RendererSurfaceStateUserData};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::signals::{Signal, Signals};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{channel, EventLoop, Interest, Mode as CMode, PostAction};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Display;
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
use smithay::desktop::PopupManager;
use smithay::utils::{Logical, Point, Rectangle, Size};

use input::cursor;
use render::PlaneDraw;
use scene::MemberId;
use state::{now_ns, spawn_client, DebugPanels, HoldPolicy, PanelSwapchain, TexRef, Zxr, PANEL_SHRINK_TICKS};
use xr::math;
use xr::{FrameTick, QuadLayer, XrCore};

const USAGE: &str = "zxr [--socket NAME] [--control PATH] [--spawn CMD]... [--frames N] [--journal PATH] [--drm-node PATH] [--xwayland DISPLAY] [--debug-panels projection] [--debug-hold replacement|tick|callback|fence] [--overlay PLACEMENT]\n       zxr ctl SOCKET COMMAND...";

struct Args {
    socket: Option<String>,
    control: Option<String>,
    spawn: Vec<String>,
    frames: Option<u64>,
    journal: Option<std::path::PathBuf>,
    drm_node: Option<String>,
    xwayland: Option<String>,
    debug_panels: DebugPanels,
    hold: HoldPolicy,
    overlay: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { socket: None, control: None, spawn: Vec::new(), frames: None, journal: None, drm_node: None, xwayland: None, debug_panels: DebugPanels::Auto, hold: HoldPolicy::Replacement, overlay: None };
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
            "--overlay" => a.overlay = Some(val()?.parse().map_err(|e| format!("--overlay PLACEMENT: {e}"))?),
            "--debug-panels" => {
                a.debug_panels = match val()?.as_str() {
                    "auto" => DebugPanels::Auto,
                    "projection" => DebugPanels::Projection,
                    other => return Err(format!("--debug-panels: {other} (auto|projection)")),
                }
            }
            "--debug-hold" => {
                a.hold = match val()?.as_str() {
                    "replacement" => HoldPolicy::Replacement,
                    "tick" => HoldPolicy::Tick,
                    "callback" => HoldPolicy::Callback,
                    "fence" => HoldPolicy::Fence,
                    other => return Err(format!("--debug-hold: {other} (replacement|tick|callback|fence)")),
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

    let (mut xr, vk) = XrCore::new(&loader, "zxr", args.overlay)?;
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
    // the wearer's preferences and the input calibrations (specs/settings-schema.md; research/73 §6):
    // resolved in-process from the artifact and the per-user store, the store directory watched
    // on this loop — before the stages are built, so their first `*Cfg` is the resolved one
    settings::install(&mut st, &event_loop.handle());
    // the input chain (spatial-input §1a): the spine installs the R0 head-ray floor in the seat
    // slot; the stages replace it as they land
    st.input.chain.set(input::Slot::Seat, Box::new(input::HeadFloor));
    st.input.chain.set(input::Slot::Stabilize, Box::new(input::stabilize::Stabilize::new()));
    st.input.chain.set(input::Slot::Hit, Box::new(input::hit::HitStage::new()));
    // the stages above targeting (spatial-input §1a lines 113-117): the reserved system input
    // first of all (native-openxr-apps §6), then the greeter/lock gate and presence (ADR 0007),
    // then the accessibility transforms (§13)
    st.input.chain.set(input::Slot::Reserved, Box::new(input::reserved::Reserved::default()));
    st.input.chain.set(input::Slot::Mode, Box::new(input::mode::ModeGate::default()));
    st.input.chain.set(input::Slot::A11y, Box::new(input::a11y::A11y::default()));
    // the tier arbiter (spatial-input §3, ruled; ADR 0013 amendment items 1, 2, 4): one targeting
    // kind at a time, gaze → held controller → hand → head, direct touch overriding a ray inside
    // WiVRn's 0.18/0.22 band, and no transition mid-gesture
    st.input.chain.set(input::Slot::Tier, Box::new(input::tier::TierStage::new()));
    // the action spaces become scene frames the batched locate finds (spatial-input §2; actions.rs)
    input::actions::register_frames(&mut st);
    // lane C: the seat stage — wl_touch / wl_pointer transports, cursors, emphasis (spatial-input
    // §5, §7, §4); keeps the head-ray floor for `Head` samples until the Hit stage produces hits
    let seat_stage = input::seat::SeatStage::new(&mut st);
    st.input.chain.set(input::Slot::Seat, Box::new(seat_stage));
    // lane F: the IM stage (spatial-input §1a line 129, §12)
    st.input.chain.set(input::Slot::Im, Box::new(input::text::ImStage));
    tracing::info!(stages = ?st.input.chain.names(), "input chain");
    st.hold = args.hold;
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

    // ---- lane F: peripheral intake on the state loop (spatial-input §1a lines 77, 80) ----
    // libinput over a libseat session (absent nested on a host: logged, continued) and the EIS
    // server socket for libei sender clients; both queue `Sample`s drained at the next tick.
    {
        let handle = event_loop.handle();
        match input::libinput::start(&mut st, &handle) {
            Ok(true) => {}
            Ok(false) => tracing::info!("no libinput intake this run"),
            Err(e) => tracing::warn!("libinput intake failed: {e}"),
        }
        if let Err(e) = input::ei::start(&mut st, &handle) {
            tracing::warn!("EIS server failed: {e}");
        }
    }
    // ---- end lane F ----

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
    let journal = format!("{}{}\n", st.journal.render(now_ns()), input::focus::render_counters(&st));
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

/// One member's surface tree, walked this tick because its panel is dirty (or it overflowed
/// into the projection pass): textures brought current and held for this slot (spec §5a: only
/// these members hold client buffers).
struct MemberTree {
    member: MemberId,
    geo: Rectangle<i32, Logical>,
    /// (texture, location relative to the toplevel origin, logical size), tree order
    items: Vec<(TexRef, Point<i32, Logical>, Size<i32, Logical>)>,
    /// geometry ∪ every surface rect, relative to the toplevel origin
    bounds: Rectangle<i32, Logical>,
}

/// Walk one member's surface tree (toplevel + subsurfaces + popups), bringing textures current.
fn walk_member(st: &mut Zxr, id: MemberId, frame: u64) -> Option<MemberTree> {
    let (window, geo) = {
        let m = st.scene.get(id)?;
        (m.m.window.clone(), m.m.window.geometry())
    };
    let root = window.toplevel().map(|t| t.wl_surface().clone())?;
    let mut surfaces: Vec<(WlSurface, Point<i32, Logical>, Size<i32, Logical>)> = Vec::new();
    collect_tree(&root, (0, 0).into(), &mut surfaces);
    for (popup, offset) in PopupManager::popups_for_surface(&root) {
        let loc = geo.loc + offset - popup.geometry().loc;
        collect_tree(popup.wl_surface(), loc, &mut surfaces);
    }
    let mut bounds = geo;
    let mut items = Vec::with_capacity(surfaces.len());
    for (surface, loc, size) in surfaces {
        let Some(tex) = st.update_surface_texture(&surface, frame) else { continue };
        bounds = bounds.merge(Rectangle::new(loc, size));
        items.push((tex, loc, size));
    }
    Some(MemberTree { member: id, geo, items, bounds })
}

/// The head pose the scene's `Views` frame takes: the midpoint of the two eyes, view 0's
/// orientation (spec §5a — no `xrLocateSpace` for VIEW).
fn head_pose(views: &[openxr::View]) -> openxr::Posef {
    let a = views[0].pose;
    let b = views.get(1).map(|v| v.pose).unwrap_or(a);
    openxr::Posef { orientation: a.orientation, position: openxr::Vector3f { x: (a.position.x + b.position.x) * 0.5, y: (a.position.y + b.position.y) * 0.5, z: (a.position.z + b.position.z) * 0.5 } }
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

    // 1. views for the predicted display time → the head frame; the head ray drives the gaze
    //    pointer. Frames the views do not give are located in one call, only when they exist.
    let views = st.xr.locate_views(time)?;
    let head = head_pose(&views);
    let head_id = st.scene.head;
    st.scene.set_frame_pose(head_id, head, true);
    {
        let ids: Vec<scene::FrameId> = st.scene.xr_frames().map(|(id, _)| id).collect();
        if !ids.is_empty() {
            let mut located: Vec<(openxr::Posef, bool)> = Vec::with_capacity(ids.len());
            {
                let Zxr { scene, xr, .. } = &mut *st;
                let spaces: Vec<&openxr::Space> = scene.xr_frames().map(|(_, s)| s).collect();
                xr.locate_spaces(&spaces, time, &mut located)?;
            }
            for (id, (pose, valid)) in ids.iter().zip(located) {
                st.scene.set_frame_pose(*id, pose, valid);
            }
            st.journal.locate_spaces_ticks += 1;
        }
    }
    // the input module (spatial-input §1a): presence event → mode stage; the head sample and the
    // XR action samples through the chain; libinput/EI/injector samples queued since the last tick
    if let Some(p) = st.xr.presence_event.take() {
        st.input.set_present(p);
        st.journal.input_presence_changes += 1;
    }
    input::tick(st, head, time, now_ns());

    // 2. this frame's slot: wait for its previous submission, release what it sampled
    let slot = (tick.frame_id % 2) as usize;
    st.slot = slot;
    st.renderer.wait_slot(slot)?;
    let gpu_ns = if st.journal.last_tick_submitted { st.renderer.read_gpu_time() } else { None };
    st.release_held(slot, tick.frame_id);
    // research/69 `--debug-hold tick`: buffers of non-sampled surfaces held since the last tick
    st.release_held_tick(tick.frame_id);

    // quiet mode (native-openxr-apps.md §4; spec §7 rev 3.2): the frame-loop round trips and
    // nothing else — no compose, no frustum, no sort, no passes, zero layers.
    if st.quiet {
        st.journal.quiet_frames += 1;
        st.journal.last_tick_submitted = false;
        st.xr.end_frame(time, None)?;
        close_event_latency(st);
        let now = Duration::from_millis(st.now_ms() as u64);
        let output = st.output.clone();
        let mut called_back: Vec<scene::MemberId> = Vec::new();
        {
            let Zxr { scene, journal, .. } = &mut *st;
            for (id, m) in scene.iter_mut() {
                if m.m.mapped() && tick.frame_id.saturating_sub(m.m.last_frame_callback) >= FALLBACK_TICKS {
                    m.m.window.send_frame(&output, now, Some(Duration::ZERO), |_, _| Some(output.clone()));
                    m.m.last_frame_callback = tick.frame_id;
                    journal.frame_callbacks += 1;
                    journal.frame_callbacks_occluded += 1;
                    called_back.push(id);
                }
            }
        }
        for id in called_back {
            st.release_held_callback(id, tick.frame_id);
        }
        let wake_to_end = now_ns().saturating_sub(tick.woke_at_ns);
        st.journal.record_frame(true, gpu_ns, wake_to_end, wake_to_end > tick.predicted_display_period.as_nanos().max(1) as u64);
        st.xr.calls.wait_frame.add(tick.wait_ns);
        st.journal.calls = st.xr.calls.clone();
        return finish_frame(st);
    }

    // 3. the flatten (spec §5a): every mapped 2D member composed once; the quad budget by band
    //    priority; overflow to the projection pass. The scratch is taken out so the list can be
    //    read while members are mutated below, and put back at the end.
    let head_pos = [head.position.x, head.position.y, head.position.z];
    // one quad is reserved for the cursor whenever there is a cursor layer (spatial-input §7: one
    // cursor element — the ring, the client's image, or both in one panel; research/70 §9)
    let cursor_layers = usize::from(st.input.cursor_layer.is_some());
    let budget = if st.debug_panels == DebugPanels::Projection { 0 } else { st.quad_budget().saturating_sub(cursor_layers) };
    let mut submit = std::mem::take(&mut st.scene.submit);
    st.scene.flatten_into(&mut submit, head_pos, budget, |p| p.presentable());
    let depth = st.depth_content_present(!submit.overflow.is_empty());
    st.journal.members_composed += (submit.quads.len() + submit.overflow.len()) as u64;
    st.journal.quads_submitted += submit.quads.len() as u64;
    st.journal.overflow += submit.overflow.len() as u64;

    // 4. dirty members only: walk the tree, size the panel (grow-only, lazy shrink), record a
    //    pass. A member whose tree did not commit is not touched and holds no buffer. Overflow
    //    members are walked every tick they overflow — they are drawn per tick.
    let mut dirty: Vec<MemberTree> = Vec::new();
    for q in &submit.quads {
        let due = {
            let Some(m) = st.scene.get_mut(q.member) else { continue };
            // a lazy shrink that came due forces one pass so the smaller image gets content
            if let Some(p) = &m.m.panel {
                if p.shrink_since.map(|s| tick.frame_id.saturating_sub(s) >= PANEL_SHRINK_TICKS).unwrap_or(false) {
                    m.m.dirty = true;
                }
            }
            m.m.dirty || m.m.panel.as_ref().map(|p| !p.has_image).unwrap_or(true)
        };
        if !due {
            continue;
        }
        let Some(tree) = walk_member(st, q.member, tick.frame_id) else { continue };
        let (w, h) = (tree.bounds.size.w.max(1) as u32, tree.bounds.size.h.max(1) as u32);
        // panel swapchain lifecycle (spec §5a)
        let old = st.scene.get_mut(q.member).and_then(|m| m.m.panel.take());
        let panel = match old {
            None => {
                let sc = st.xr.create_panel_swapchain(w, h)?;
                let target = st.renderer.make_panel_target(sc.extent, st.xr.color_format, &sc.images)?;
                st.journal.panel_swapchains_created += 1;
                PanelSwapchain { sc, target, has_image: false, bounds: tree.bounds, shrink_since: None, passes: 0 }
            }
            Some(mut ps) => {
                let (ew, eh) = (ps.sc.extent.width, ps.sc.extent.height);
                if w > ew || h > eh {
                    // grow: a new image at the new bounds (the old one dies after its fences)
                    st.retire_panel(ps);
                    let sc = st.xr.create_panel_swapchain(w, h)?;
                    let target = st.renderer.make_panel_target(sc.extent, st.xr.color_format, &sc.images)?;
                    st.journal.panel_swapchains_created += 1;
                    st.journal.panel_swapchains_grown += 1;
                    PanelSwapchain { sc, target, has_image: false, bounds: tree.bounds, shrink_since: None, passes: 0 }
                } else if w < ew || h < eh {
                    match ps.shrink_since {
                        Some(since) if tick.frame_id.saturating_sub(since) >= PANEL_SHRINK_TICKS => {
                            st.retire_panel(ps);
                            let sc = st.xr.create_panel_swapchain(w, h)?;
                            let target = st.renderer.make_panel_target(sc.extent, st.xr.color_format, &sc.images)?;
                            st.journal.panel_swapchains_created += 1;
                            st.journal.panel_swapchains_shrunk += 1;
                            PanelSwapchain { sc, target, has_image: false, bounds: tree.bounds, shrink_since: None, passes: 0 }
                        }
                        Some(_) => {
                            ps.bounds = tree.bounds;
                            ps
                        }
                        None => {
                            ps.shrink_since = Some(tick.frame_id);
                            ps.bounds = tree.bounds;
                            ps
                        }
                    }
                } else {
                    ps.shrink_since = None;
                    ps.bounds = tree.bounds;
                    ps
                }
            }
        };
        if let Some(m) = st.scene.get_mut(q.member) {
            m.m.panel = Some(panel);
        }
        dirty.push(tree);
    }
    let mut overflow_trees: Vec<MemberTree> = Vec::with_capacity(submit.overflow.len());
    for q in &submit.overflow {
        if let Some(t) = walk_member(st, q.member, tick.frame_id) {
            overflow_trees.push(t);
        }
    }
    st.journal.members_dirty += dirty.len() as u64;

    // the cursor panel (spatial-input §7; research/70 §9): one fixed CURSOR_PX² swapchain, grown
    // only when a client image needs more room around its hotspot; drawn into only when the
    // layer's content key changes — the ring once, a `set_cursor` surface on its commits, a
    // `cursor-shape-v1` name when the name changes — never per pointer motion (the cursor-plane
    // shape). The layer's centre is the hotspot, so the quad is centred on the pointer point.
    let mut cursor_draw: Option<CursorDraw> = None;
    // whether the panel's content is presentable this tick (an `Image` layer whose image has
    // no texture yet shows nothing rather than a stale panel)
    let mut cursor_visible = false;
    if let Some(layer) = st.input.cursor_layer.clone() {
        let mut image: Option<(CursorTex, Size<i32, Logical>, Point<i32, Logical>, state::CursorImageKey)> = None;
        if layer.content.has_image() {
            match layer.image.as_ref() {
                Some(smithay::input::pointer::CursorImageStatus::Surface(surf)) => {
                    if let Some(tex) = st.update_surface_texture(surf, tick.frame_id) {
                        let (commit, size) = with_renderer_surface_state(surf, |s| (s.current_commit(), s.surface_size())).map(|(c, s)| (c, s.unwrap_or_else(|| Size::from((1, 1))))).unwrap_or((Default::default(), Size::from((1, 1))));
                        let hotspot = smithay::wayland::compositor::with_states(surf, |s| s.data_map.get::<smithay::input::pointer::CursorImageSurfaceData>().map(|d| d.lock().unwrap().hotspot)).unwrap_or_default();
                        image = Some((CursorTex::Surface(tex), size, hotspot, state::CursorImageKey::Surface(smithay::reexports::wayland_server::Resource::id(surf), commit)));
                    }
                }
                Some(smithay::input::pointer::CursorImageStatus::Named(icon)) => match st.cursor_theme.lookup(*icon) {
                    Some(img) => {
                        let changed = st.cursor_named.as_ref().map(|(i, _)| *i != *icon).unwrap_or(true);
                        if changed {
                            if let Some((_, old)) = st.cursor_named.take() {
                                st.renderer.destroy_texture(old);
                            }
                            let mut tex = st.renderer.create_shm_texture(img.width, img.height)?;
                            st.renderer.upload_shm(&mut tex, &img.argb, img.width * 4)?;
                            st.cursor_named = Some((*icon, tex));
                        }
                        image = Some((CursorTex::Named, Size::from((img.width as i32, img.height as i32)), Point::from(img.hotspot), state::CursorImageKey::Named(*icon)));
                    }
                    None => st.input.cursor_named_ticks += 1,
                },
                _ => {}
            }
        }
        // what is drawable now: the ring when asked; the image only when it resolved
        let content = match (layer.content, image.is_some()) {
            (cursor::Content::RingAndImage, false) => Some(cursor::Content::Ring),
            (cursor::Content::Image, false) => None,
            (c, _) => Some(c),
        };
        if let Some(content) = content {
            let side = image.as_ref().map(|(_, s, h, _)| cursor::panel_side_for(s.w.max(1) as u32, s.h.max(1) as u32, h.x, h.y)).unwrap_or(cursor::CURSOR_PX);
            let fresh = ensure_cursor_panel(st, side)?;
            if st.ring_tex.is_none() {
                let mut tex = st.renderer.create_shm_texture(cursor::RETICLE_PX, cursor::RETICLE_PX)?;
                st.renderer.upload_shm(&mut tex, &cursor::ring_pixels(cursor::RETICLE_PX), cursor::RETICLE_PX * 4)?;
                st.ring_tex = Some(tex);
            }
            let key = state::CursorKey { content, image: image.as_ref().map(|(_, _, _, k)| k.clone()) };
            if fresh || st.cursor_key.as_ref() != Some(&key) {
                st.cursor_key = Some(key);
                cursor_draw = Some(CursorDraw { content, image: image.map(|(t, s, h, _)| (t, s, h)) });
            }
            cursor_visible = true;
        }
    }
    let mut cursor_acquired = false;
    let mut cursor_key_reset = false;

    // 5. GPU work — only if there is any: dirty panel passes, the cursor's pass and/or the projection pass
    let submit_gpu = !dirty.is_empty() || depth || cursor_draw.is_some();
    if submit_gpu {
        let Zxr { renderer, dmabuf_textures, surface_tex, journal, scene, xr, ring_tex, cursor_panel, cursor_named, .. } = &mut *st;
        let m_per_px = scene.layout.m_per_px;
        let resolve = |r: &TexRef| -> Option<&render::Texture> {
            match r {
                TexRef::Dmabuf(bid) => dmabuf_textures.get(bid).map(|(t, _)| t),
                TexRef::Surface(sid) => surface_tex.get(sid).and_then(|e| e.texture.as_ref()),
            }
        };
        // foreign-queue barriers for every dmabuf sampled this tick (panels + overflow)
        let mut foreign: Vec<ash::vk::Image> = Vec::new();
        for t in dirty.iter().chain(overflow_trees.iter()) {
            for (tex, _, _) in &t.items {
                if let Some(t) = resolve(tex) {
                    if t.dmabuf && !foreign.contains(&t.image) {
                        foreign.push(t.image);
                    }
                }
            }
        }
        if let Some(CursorDraw { image: Some((CursorTex::Surface(tex), _, _)), .. }) = cursor_draw.as_ref() {
            if let Some(t) = resolve(tex) {
                if t.dmabuf && !foreign.contains(&t.image) {
                    foreign.push(t.image);
                }
            }
        }
        let cmd = renderer.begin_frame(slot, &foreign)?;
        // panel passes: orthographic into the top-left `bounds` of the (possibly larger) image,
        // pixels → NDC (y down, as the framebuffer), textures flipped
        for t in &dirty {
            let Some(ps) = scene.get_mut(t.member).and_then(|m| m.m.panel.as_mut()) else { continue };
            let idx = xr.acquire_panel_image(&mut ps.sc)?;
            journal.panel_acquires += 1;
            let (w, h) = (t.bounds.size.w.max(1) as f32, t.bounds.size.h.max(1) as f32);
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
            renderer.record_pass_in(cmd, &ps.target, idx, ash::vk::Extent2D { width: w as u32, height: h as u32 }, &ortho, &planes, [0.0, 0.0, 0.0, 0.0]);
            ps.has_image = true;
            ps.passes += 1;
            renderer.panel_passes += 1;
            renderer.panel_bytes += (w * h) as u64 * 4 * 2;
        }
        // the cursor pass (§7, research/70 §9): the ring and/or the client image into the one
        // cursor panel, hotspot at the centre — on a content change only, never per motion
        if let (Some(cd), Some(ps)) = (cursor_draw.as_ref(), cursor_panel.as_mut()) {
            let image_tex = match cd.image.as_ref() {
                Some((CursorTex::Surface(tex), _, _)) => resolve(tex),
                Some((CursorTex::Named, _, _)) => cursor_named.as_ref().map(|(_, t)| t),
                None => None,
            };
            if cd.image.is_some() && image_tex.is_none() {
                // the surface has no texture yet: draw nothing this tick and retry next tick
                journal.stale_texture_draws += 1;
                cursor_key_reset = true;
            } else {
                let idx = xr.acquire_panel_image(&mut ps.sc)?;
                journal.panel_acquires += 1;
                cursor_acquired = true;
                let side = ps.sc.extent.width.max(1) as f32;
                let c = side * 0.5;
                let ortho = math::ortho_px(side, side);
                let mut planes: Vec<PlaneDraw<'_>> = Vec::with_capacity(2);
                if cd.content.has_ring() {
                    if let Some(ring) = ring_tex.as_ref() {
                        let r = cursor::RETICLE_PX as f32 * 0.5;
                        planes.push(PlaneDraw { model: math::model([c, c, 0.0], 0.0), half_size: [r, r], texture: ring, flip_v: true });
                    }
                }
                if let (Some((_, size, hot)), Some(texture)) = (cd.image.as_ref(), image_tex) {
                    let (w, h) = (size.w.max(1) as f32, size.h.max(1) as f32);
                    // the hotspot pixel lands on the panel centre; the image is drawn over the ring
                    planes.push(PlaneDraw { model: math::model([c - hot.x as f32 + w * 0.5, c - hot.y as f32 + h * 0.5, -0.001], 0.0), half_size: [w * 0.5, h * 0.5], texture, flip_v: true });
                }
                renderer.record_pass_in(cmd, &ps.target, idx, ash::vk::Extent2D { width: side as u32, height: side as u32 }, &ortho, &planes, [0.0, 0.0, 0.0, 0.0]);
                ps.has_image = true;
                ps.passes += 1;
                journal.cursor_passes += 1;
            }
        }
        // the projection pass: overflow members (and, from M2, volumes / environment / cutout)
        if depth {
            let projection_indices = xr.acquire_images()?;
            let flip = math::flip_y();
            for (vi, v) in views.iter().enumerate() {
                let view_proj = math::mul(&math::mul(&flip, &math::projection(v.fov, 0.05, 100.0)), &math::view(v.pose));
                let mut planes: Vec<PlaneDraw<'_>> = Vec::new();
                for (q, t) in submit.overflow.iter().zip(overflow_trees.iter()) {
                    let world = scene::model(q.world);
                    let centre = [t.geo.loc.x as f32 + t.geo.size.w as f32 * 0.5, t.geo.loc.y as f32 + t.geo.size.h as f32 * 0.5];
                    for (i, (tex, loc, size)) in t.items.iter().enumerate() {
                        let Some(texture) = resolve(tex) else {
                            journal.stale_texture_draws += 1;
                            continue;
                        };
                        let cx = (loc.x as f32 + size.w as f32 * 0.5 - centre[0]) * m_per_px;
                        let cy = -(loc.y as f32 + size.h as f32 * 0.5 - centre[1]) * m_per_px;
                        let local = math::model([cx, cy, i as f32 * 0.0005], 0.0);
                        planes.push(PlaneDraw { model: math::mul(&world, &local), half_size: [size.w as f32 * m_per_px * 0.5, size.h as f32 * m_per_px * 0.5], texture, flip_v: false });
                    }
                }
                let clear = if submit.quads.is_empty() { [0.05, 0.05, 0.08, 1.0] } else { [0.0, 0.0, 0.0, 0.0] };
                renderer.record_pass(cmd, renderer.view_target(vi), projection_indices[vi], &view_proj, &planes, clear);
            }
        }
        renderer.end_frame(slot, &foreign)?;
        // releases only after the work is queued (the runtime waits on the queue, not the CPU)
        for t in &dirty {
            if let Some(ps) = scene.get_mut(t.member).and_then(|m| m.m.panel.as_mut()) {
                xr.release_panel_image(&mut ps.sc)?;
                journal.panel_releases += 1;
            }
            if let Some(m) = scene.get_mut(t.member) {
                m.m.dirty = false;
            }
        }
        if cursor_acquired {
            if let Some(ps) = cursor_panel.as_mut() {
                xr.release_panel_image(&mut ps.sc)?;
                journal.panel_releases += 1;
            }
        }
        if depth {
            xr.release_images()?;
        }
    }
    if cursor_key_reset {
        st.cursor_key = None;
    }
    st.journal.last_tick_submitted = submit_gpu;
    if depth {
        st.journal.projection_layer_frames += 1;
    } else {
        st.journal.panels_only_frames += 1;
    }

    // 6. xrEndFrame: the projection layer (if any) first, then the quads in the flatten's order
    //    (band ascending, nearest last — painter's order, `rendering.adoc:1143-1147`)
    {
        let emphasis = st.input.emphasis;
        // `input.emphasis.strength` (spatial-input §4; the 0.15 stand-in is its default)
        let emphasis_strength = st.prefs.emphasis_strength;
        let cursor = st.input.cursor_layer.as_ref().map(|l| (l.pose, l.m_per_px));
        let Zxr { xr, scene, cursor_panel, journal, .. } = &mut *st;
        let m_per_px_plane = scene.layout.m_per_px;
        let mut quads: Vec<QuadLayer<'_>> = Vec::with_capacity(submit.quads.len());
        for q in &submit.quads {
            let Some(m) = scene.get(q.member) else { continue };
            let Some(ps) = m.m.panel.as_ref() else { continue };
            if !ps.has_image {
                continue;
            }
            // the quad is centred on the panel bounds, offset from the plane's geometry centre
            let geo = m.m.window.geometry();
            let dx = ((ps.bounds.loc.x as f32 + ps.bounds.size.w as f32 * 0.5) - (geo.loc.x as f32 + geo.size.w as f32 * 0.5)) * m_per_px_plane;
            let dy = -((ps.bounds.loc.y as f32 + ps.bounds.size.h as f32 * 0.5) - (geo.loc.y as f32 + geo.size.h as f32 * 0.5)) * m_per_px_plane;
            let p = math::pose_apply(q.world, [dx, dy, 0.0]);
            quads.push(QuadLayer {
                swapchain: &ps.sc,
                pose: openxr::Posef { orientation: q.world.orientation, position: openxr::Vector3f { x: p[0], y: p[1], z: p[2] } },
                size: [ps.bounds.size.w as f32 * m_per_px_plane, ps.bounds.size.h as f32 * m_per_px_plane],
                image_extent: [ps.bounds.size.w.max(1) as u32, ps.bounds.size.h.max(1) as u32],
                // touch-class emphasis of the targeted member (spatial-input §4)
                emphasis: emphasis.filter(|(m, _)| *m == q.member).map(|(_, e)| e).unwrap_or(0.0),
            });
        }
        // the one cursor layer (§7, research/70 §9): band 5, nearest — last in painter's order,
        // never emphasised; centred on the pointer point / hit (the hotspot is the panel centre),
        // sized so RETICLE_PX panel pixels subtend RETICLE_DEG at the point's distance
        if let (Some((pose, m_per_px)), Some(ps), true) = (cursor, cursor_panel.as_ref(), cursor_visible) {
            if ps.has_image {
                let side = ps.sc.extent.width.max(1);
                let size_m = m_per_px * side as f32;
                quads.push(QuadLayer { swapchain: &ps.sc, pose, size: [size_m, size_m], image_extent: [side, side], emphasis: 0.0 });
                journal.cursor_layers += 1;
            }
        }
        xr.end_frame_with_quads(time, if depth { Some(&views) } else { None }, &quads, emphasis_strength)?;
    }
    close_event_latency(st);

    // 7. frame callbacks: once per refresh, after xrEndFrame (§6.6). Visibility-gated
    //    (research/65 §4.2; niri `niri.rs:5178-5208`, KWin `item.cpp:739-751`, mutter
    //    `meta-wayland.c:182-219` converge): a member in either view's frustum gets its callback
    //    every tick; one out of view gets it on a fallback cadence (niri: 995 ms) so a client
    //    waiting on a callback never stalls, but stops driving the GPU at display rate.
    let now = Duration::from_millis(st.now_ms() as u64);
    let output = st.output.clone();
    let mut called_back: Vec<MemberId> = Vec::new();
    {
        let Zxr { scene, journal, .. } = &mut *st;
        for q in submit.quads.iter().chain(submit.overflow.iter()) {
            let Some(m) = scene.get_mut(q.member) else { continue };
            let model = scene::model(q.world);
            let visible = views.iter().any(|v| plane_in_view(v, &model, [q.size[0] * 0.5, q.size[1] * 0.5]));
            let due = tick.frame_id.saturating_sub(m.m.last_frame_callback) >= FALLBACK_TICKS;
            if visible {
                journal.frame_callbacks_visible += 1;
            } else {
                journal.frame_callbacks_occluded += 1;
            }
            if visible || due {
                m.m.window.send_frame(&output, now, Some(Duration::ZERO), |_, _| Some(output.clone()));
                m.m.last_frame_callback = tick.frame_id;
                journal.frame_callbacks += 1;
                called_back.push(q.member);
            }
        }
        // hidden members are not in the flatten: fallback cadence only (research/69 A/B)
        for (id, m) in scene.iter_mut() {
            if m.m.mapped() && m.m.hidden && tick.frame_id.saturating_sub(m.m.last_frame_callback) >= FALLBACK_TICKS {
                m.m.window.send_frame(&output, now, Some(Duration::ZERO), |_, _| Some(output.clone()));
                m.m.last_frame_callback = tick.frame_id;
                journal.frame_callbacks += 1;
                journal.frame_callbacks_occluded += 1;
                called_back.push(id);
            }
        }
    }
    st.scene.submit = submit;
    for id in called_back {
        st.release_held_callback(id, tick.frame_id);
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
    st.journal.panel_swapchains = st.scene.iter().filter(|(_, m)| m.m.panel.is_some()).count() as u64;
    finish_frame(st)
}

/// Fallback frame-callback cadence for members out of view (niri: 995 ms).
const FALLBACK_TICKS: u64 = 60;

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
    if !st.retired_panels.is_empty() {
        st.flush_retired_panels(st.frame_id);
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
            format!("focused {:?}", f.map(|m| m.0.index()))
        }
        List => {
            let mut s = String::new();
            for (id, m) in st.scene.iter() {
                let title = m.m.window.toplevel().map(|t| smithay::wayland::compositor::with_states(t.wl_surface(), |s| s.data_map.get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>().unwrap().lock().unwrap().title.clone().unwrap_or_default())).unwrap_or_default();
                let g = m.m.window.geometry();
                let w = st.scene.world_pose(id).unwrap_or(m.local);
                let band = st.scene.band(id).unwrap_or(0);
                let panel = m.m.panel.as_ref().map(|p| format!("{}x{}", p.sc.extent.width, p.sc.extent.height)).unwrap_or_else(|| "-".into());
                s.push_str(&format!(
                    "{}{} band={band} place={} {}x{} pos=({:.2},{:.2},{:.2}) mapped={} dirty={} urgent={} panel={panel} {title}\n",
                    id.0.index(),
                    if Some(id) == st.scene.focused { "*" } else { " " },
                    m.place.0.index(),
                    g.size.w,
                    g.size.h,
                    w.position.x,
                    w.position.y,
                    w.position.z,
                    m.m.mapped(),
                    m.m.dirty
,
                    m.m.urgent
                ));
            }
            // the one cursor layer (spatial-input §7; research/70 §9): content, panel, passes
            let content = st.input.cursor_layer.as_ref().map(|l| format!("{:?} at=({:.2},{:.2},{:.2}) side_m={:.3}", l.content, l.pose.position.x, l.pose.position.y, l.pose.position.z, l.m_per_px * st.cursor_panel.as_ref().map(|p| p.sc.extent.width).unwrap_or(cursor::CURSOR_PX) as f32)).unwrap_or_else(|| "none".into());
            let panel = st.cursor_panel.as_ref().map(|p| format!("{}x{} passes={}", p.sc.extent.width, p.sc.extent.height, p.passes)).unwrap_or_else(|| "-".into());
            let inputs = st.input.cursor_inputs.map(|i| format!("targeting={:?} owner={:?} on_plane={} reticle={} typing={} client={}", i.targeting, i.owner, i.pointer_on_plane, i.reticle, i.hidden_typing, i.client_name)).unwrap_or_default();
            s.push_str(&format!("cursor: layer={content} panel={panel} swapchains_created={} layers_submitted={} {inputs}\n", st.journal.cursor_swapchains_created, st.journal.cursor_layers));
            // the settings picture (settings.rs): where it came from and how often it moved
            s.push_str(&format!(
                "settings: artifact={} keys={} generation={} reloads={} invalid={} cursor.ray={} pointer.gain={} dwell={} density_px_per_cm={:.1} targeting={} dominant={} xkb={}/{} repeat={}/{} theme={}@{} warp={} long_press_ms={}\n",
                if st.settings.is_some() { st.prefs.artifact_generation.as_str() } else { "none (built-in defaults)" },
                st.journal.settings_keys,
                st.prefs.generation,
                st.journal.settings_reloads,
                st.journal.settings_invalid,
                st.prefs.cursor_ray,
                st.prefs.pointer_gain,
                st.prefs.dwell_enabled,
                st.prefs.wm_density_px_per_cm,
                st.prefs.targeting_source,
                st.prefs.hand_dominant,
                if st.prefs.xkb_layout.is_empty() { "default" } else { st.prefs.xkb_layout.as_str() },
                st.prefs.xkb_variant,
                st.prefs.repeat_delay_ms,
                st.prefs.repeat_rate_hz,
                st.cursor_theme.name(),
                st.cursor_theme.size(),
                st.prefs.pointer_warp,
                st.prefs.system_long_press_ms
            ));
            s.trim_end().to_string()
        }
        Journal => format!("{}{}", st.journal.render(now_ns()), input::focus::render_counters(st)).trim_end().to_string(),
        Quiet(on) => {
            st.set_quiet(on);
            format!("quiet {}", if on { "on" } else { "off" })
        }
        Primary(on) => {
            st.primary_changed(on);
            format!("primary {} quiet={} keep_planes={}", if on { "on" } else { "off" }, st.quiet, st.prefs.games_keep_planes)
        }
        Mode(m) => {
            let mode = match m.as_str() {
                "normal" => Some(input::Mode::Normal),
                "greeter" => Some(input::Mode::Greeter),
                "locked" | "lock" => Some(input::Mode::Locked),
                _ => None,
            };
            match mode {
                Some(mode) => {
                    st.input.mode = mode;
                    format!("mode {mode:?}")
                }
                None => format!("error unknown mode {m}"),
            }
        }
        A11y(key, val) => match (key.as_str(), val.as_str()) {
            ("dwell", v) => {
                st.input.a11y_dwell = Some(v == "on" || v == "1");
                format!("a11y dwell {v}")
            }
            ("gain", v) => match v.parse::<f64>() {
                Ok(g) => {
                    st.input.a11y_gain = Some(g);
                    format!("a11y gain {g}")
                }
                Err(_) => format!("error a11y gain {v}"),
            },
            _ => format!("error unknown a11y setting {key}"),
        },
        Cursor(key, val) => match (key.as_str(), cursor::RayCursor::parse(&val), cursor::Scale::parse(&val)) {
            ("ray", Some(r), _) => {
                st.input.cursor_ray = Some(r);
                format!("cursor ray {val}")
            }
            ("scale", _, Some(s)) => {
                st.input.cursor_scale = Some(s);
                format!("cursor scale {val}")
            }
            _ => format!("error cursor {key} {val} (ray both|image|ring; scale angle|plane)"),
        },
        Present(on) => {
            st.input.set_present(on);
            st.journal.input_presence_changes += 1;
            format!("present {}", if on { "on" } else { "off" })
        }
        Source(cmd) => {
            let now = now_ns();
            // dispatched now, like a device event (spatial-input §1a: per-event dispatch on the loop)
            let mut out: Vec<input::Sample> = Vec::with_capacity(1);
            let head = st.input.head;
            match st.input.injector.apply(&cmd, now, head, &mut out) {
                Ok(r) => {
                    for s in out {
                        input::dispatch(st, s, now);
                    }
                    format!("dispatched {r}")
                }
                Err(e) => format!("error {e}"),
            }
        }
        Hide(on) => match st.scene.focused {
            Some(id) => {
                if let Some(m) = st.scene.get_mut(id) {
                    m.m.hidden = on;
                    // returning to view: the panel must be repainted from the latest buffer
                    if !on {
                        m.m.dirty = true;
                    }
                }
                let quiet = st.quiet;
                st.set_suspended(id, on || quiet);
                format!("hide {}", if on { "on" } else { "off" })
            }
            None => "no focused member".into(),
        },
        Quit => {
            st.loop_signal.stop();
            "bye".into()
        }
        Close => match st.scene.focused().and_then(|m| m.m.window.toplevel().cloned()) {
            Some(t) => {
                t.send_close();
                "closed".into()
            }
            None => "no focus".into(),
        },
        Resize(w, h) => match st.scene.focused().and_then(|m| m.m.window.toplevel().cloned()) {
            Some(t) => {
                t.with_pending_state(|s| s.size = Some((w, h).into()));
                t.send_pending_configure();
                format!("configure {w}x{h}")
            }
            None => "no focus".into(),
        },
        // `set_local` through the mutation API (spec §5a): the delta is applied in the place's frame
        Move(dx, dy, dz) => match st.scene.focused.and_then(|id| st.scene.get(id).map(|m| (id, m.local))) {
            Some((id, mut local)) => {
                local.position.x += dx;
                local.position.y += dy;
                local.position.z += dz;
                st.scene.set_local(id, local);
                format!("pos=({:.2},{:.2},{:.2})", local.position.x, local.position.y, local.position.z)
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

/// Close this tick's event→`xrEndFrame` interval (the M1 gate's trigger number, spec §12): the
/// oldest event's age (mean/max) and the per-event mean over every event the tick carried.
fn close_event_latency(st: &mut Zxr) {
    let end = now_ns();
    if let Some(t0) = st.input.tick_oldest_event_ns.take() {
        let d = end.saturating_sub(t0);
        st.journal.input_event_to_end_n += 1;
        st.journal.input_event_to_end_ns_total += d;
        st.journal.input_event_to_end_ns_max = st.journal.input_event_to_end_ns_max.max(d);
    }
    let n = std::mem::take(&mut st.input.tick_event_count);
    let sum = std::mem::take(&mut st.input.tick_event_time_sum_ns);
    if n > 0 {
        st.journal.input_event_to_end_per_event_n += n;
        st.journal.input_event_to_end_per_event_ns_total += (end as u128 * n as u128).saturating_sub(sum) as u64;
    }
}

/// Where the client cursor pass samples from this tick.
enum CursorTex {
    Surface(TexRef),
    Named,
}

/// What the cursor pass draws this tick: the content shape and, for an image, its texture, size
/// and hotspot (spatial-input §7; research/70 §9).
struct CursorDraw {
    content: cursor::Content,
    image: Option<(CursorTex, Size<i32, Logical>, Point<i32, Logical>)>,
}

/// The one cursor panel, at least `side`² — kept when it is already that large (a smaller image
/// is drawn into the fixed panel around its hotspot; the DRM cursor plane's shape), grown only
/// when a client image needs more, never shrunk. Returns whether the panel is fresh (needs its
/// first pass). Counted in `cursor_swapchains_created`, not with the members' panels.
fn ensure_cursor_panel(st: &mut Zxr, side: u32) -> Result<bool, String> {
    let side = side.max(cursor::CURSOR_PX);
    if let Some(p) = st.cursor_panel.as_ref() {
        if p.sc.extent.width >= side && p.sc.extent.height >= side {
            return Ok(false);
        }
    }
    if let Some(old) = st.cursor_panel.take() {
        st.retire_panel(old);
    }
    let sc = st.xr.create_panel_swapchain(side, side)?;
    let target = st.renderer.make_panel_target(sc.extent, st.xr.color_format, &sc.images)?;
    st.journal.cursor_swapchains_created += 1;
    let bounds = Rectangle::new(Point::from((0, 0)), Size::from((side as i32, side as i32)));
    st.cursor_panel = Some(PanelSwapchain { sc, target, has_image: false, bounds, shrink_since: None, passes: 0 });
    Ok(true)
}
