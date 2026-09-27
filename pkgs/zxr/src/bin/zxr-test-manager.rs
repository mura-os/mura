//! A scripted `zxr_window_management_v1` client — the seam's proof harness, not a shipping
//! manager (window-workspace-management §11; the nested gate in specs/zxr-core.md §12).
//!
//! Built only with `--features test-manager` (the product closure never carries it). It binds
//! the global, prints every event it hears as `ev: …` lines, works through its argv *steps* in
//! the sequences river's protocol shape gives it (manage steps inside `manage_start` …
//! `manage_finish`, rendering steps inside `render_start` … `render_finish`), then prints the
//! picture it holds and exits — which is the disconnect the contract is about.
//!
//! Steps (each one argv word):
//!   assign:W:P|none      manage   — window W (announce order) to place P, or unassigned
//!   focus:W:stale|fresh  manage   — `focus` with a made-up serial, or the last `interaction`'s
//!                                   (waits for one to arrive)
//!   maximize:W:on|off    manage
//!   pose:W:x,y,z         render   — `set_pose`, identity rotation at the place-local point
//!   hide:W / show:W      render
//!   engine:P:custom|free render
//!   badpose:W            render   — a non-rigid matrix (the compositor must post invalid_pose)
//!   wait:N                        — let N further sequences pass
//!   hold                          — never finish; serve sequences until killed
//!   stall                         — stop answering sequences (the unresponsive contract)
//!
//! Exit: 0 steps done · 2 `unavailable` (another manager holds the seam) · 3 no global ·
//! 4 protocol error from the compositor.

use std::collections::VecDeque;
use std::process::exit;

use wayland_client::protocol::{wl_registry, wl_registry::WlRegistry};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};

#[allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code, clippy::all)]
mod protocol {
    use wayland_client;

    pub mod __interfaces {
        wayland_scanner::generate_interfaces!("../../protocols/zxr-window-management-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("../../protocols/zxr-window-management-v1.xml");
}

use protocol::zxr_managed_place_v1::{self, ZxrManagedPlaceV1};
use protocol::zxr_managed_window_v1::{self, ZxrManagedWindowV1};
use protocol::zxr_window_manager_v1::{self, ZxrWindowManagerV1};

#[derive(Debug, Clone)]
enum Step {
    Assign(usize, Option<usize>),
    Focus(usize, bool),
    Maximize(usize, bool),
    Pose(usize, [f32; 3]),
    Hide(usize, bool),
    Engine(usize, u32),
    BadPose(usize),
    Wait(u32),
    Hold,
    Stall,
}

impl Step {
    /// Which sequence a step belongs in: `true` manage, `false` render; `None` either.
    fn management(&self) -> Option<bool> {
        match self {
            Step::Assign(..) | Step::Focus(..) | Step::Maximize(..) => Some(true),
            Step::Pose(..) | Step::Hide(..) | Step::Engine(..) | Step::BadPose(..) => Some(false),
            Step::Wait(_) | Step::Hold | Step::Stall => None,
        }
    }

    fn parse(s: &str) -> Option<Step> {
        let mut it = s.split(':');
        let verb = it.next()?;
        let a = it.next();
        let b = it.next();
        Some(match verb {
            "assign" => Step::Assign(a?.parse().ok()?, if b? == "none" { None } else { Some(b?.parse().ok()?) }),
            "focus" => Step::Focus(a?.parse().ok()?, b? == "fresh"),
            "maximize" => Step::Maximize(a?.parse().ok()?, b? == "on"),
            "pose" => {
                let v: Vec<f32> = b?.split(',').filter_map(|x| x.parse().ok()).collect();
                if v.len() != 3 {
                    return None;
                }
                Step::Pose(a?.parse().ok()?, [v[0], v[1], v[2]])
            }
            "hide" => Step::Hide(a?.parse().ok()?, true),
            "show" => Step::Hide(a?.parse().ok()?, false),
            "engine" => Step::Engine(a?.parse().ok()?, if b? == "custom" { 4 } else { 0 }),
            "badpose" => Step::BadPose(a?.parse().ok()?),
            "wait" => Step::Wait(a?.parse().ok()?),
            "hold" => Step::Hold,
            "stall" => Step::Stall,
            _ => return None,
        })
    }
}

struct Window {
    obj: ZxrManagedWindowV1,
    app_id: String,
    title: String,
    place: Option<usize>,
    pos: [f32; 3],
    closed: bool,
}

struct Place {
    obj: ZxrManagedPlaceV1,
    engine: u32,
    members: Vec<usize>,
}

#[derive(Default)]
struct State {
    manager: Option<ZxrWindowManagerV1>,
    windows: Vec<Window>,
    places: Vec<Place>,
    capabilities: u32,
    limits: Option<(f64, f64, f64)>,
    last_interaction: Option<u32>,
    steps: VecDeque<Step>,
    /// how many sequences still to let pass before the next step (a `wait`)
    waiting: u32,
    done: bool,
    stalled: bool,
    unavailable: bool,
    finished: bool,
    sequences: u32,
}

fn pose_bytes(p: [f32; 3]) -> Vec<u8> {
    let mut m = [0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m[12] = p[0];
    m[13] = p[1];
    m[14] = p[2];
    m.iter().flat_map(|f| f.to_ne_bytes()).collect()
}

fn translation(b: &[u8]) -> [f32; 3] {
    let f = |i: usize| {
        let c = &b[i * 4..i * 4 + 4];
        f32::from_ne_bytes([c[0], c[1], c[2], c[3]])
    };
    if b.len() == 64 {
        [f(12), f(13), f(14)]
    } else {
        [f32::NAN; 3]
    }
}

impl State {
    fn window_index(&self, w: &ZxrManagedWindowV1) -> Option<usize> {
        self.windows.iter().position(|x| x.obj == *w)
    }
    fn place_index(&self, p: &ZxrManagedPlaceV1) -> Option<usize> {
        self.places.iter().position(|x| x.obj == *p)
    }

    /// Run the steps that belong in this sequence; stop at the first that does not (or that
    /// waits on something not yet here). Returns false if the sequence must stay open.
    fn run_steps(&mut self, management: bool) -> bool {
        loop {
            let Some(step) = self.steps.front().cloned() else { return true };
            match step {
                Step::Wait(n) => {
                    // counts manage sequences let pass
                    if management {
                        if self.waiting == 0 {
                            self.waiting = n;
                        }
                        self.waiting -= 1;
                        if self.waiting == 0 {
                            self.steps.pop_front();
                        }
                    }
                    return true;
                }
                Step::Hold => return true,
                Step::Stall => {
                    self.stalled = true;
                    println!("step: stall — not finishing sequence");
                    return false;
                }
                s => {
                    if s.management() != Some(management) {
                        return true;
                    }
                    if let Step::Focus(w, true) = s {
                        if self.last_interaction.is_none() {
                            println!("step: focus:{w}:fresh waits for an interaction");
                            return true;
                        }
                    }
                    self.steps.pop_front();
                    self.apply(s);
                }
            }
        }
    }

    fn apply(&mut self, s: Step) {
        let win = |i: usize| -> Option<&Window> { self.windows.get(i).filter(|w| !w.closed) };
        match s {
            Step::Assign(w, p) => {
                let Some(win) = win(w) else { println!("step: assign — no window {w}"); return };
                let place = p.and_then(|p| self.places.get(p)).map(|p| &p.obj);
                win.obj.assign(place);
                println!("step: assign window {w} -> place {}", p.map(|p| p.to_string()).unwrap_or_else(|| "none".into()));
            }
            Step::Focus(w, fresh) => {
                let Some(win) = win(w) else { println!("step: focus — no window {w}"); return };
                let serial = if fresh { self.last_interaction.unwrap_or(0) } else { 0xC0FFEE };
                win.obj.focus(serial);
                println!("step: focus window {w} serial={serial} ({})", if fresh { "fresh" } else { "stale" });
            }
            Step::Maximize(w, on) => {
                let Some(win) = win(w) else { println!("step: maximize — no window {w}"); return };
                win.obj.set_maximized(on as u32);
                println!("step: maximize window {w} {on}");
            }
            Step::Pose(w, p) => {
                let Some(win) = win(w) else { println!("step: pose — no window {w}"); return };
                win.obj.set_pose(pose_bytes(p));
                println!("step: set_pose window {w} -> ({:.2},{:.2},{:.2})", p[0], p[1], p[2]);
            }
            Step::BadPose(w) => {
                let Some(win) = win(w) else { println!("step: badpose — no window {w}"); return };
                let mut b = pose_bytes([0.0; 3]);
                b[0..4].copy_from_slice(&2.0f32.to_ne_bytes()); // x axis of length 2: not rigid
                win.obj.set_pose(b);
                println!("step: set_pose window {w} non-rigid");
            }
            Step::Hide(w, on) => {
                let Some(win) = win(w) else { println!("step: hide — no window {w}"); return };
                if on {
                    win.obj.hide();
                } else {
                    win.obj.show();
                }
                println!("step: {} window {w}", if on { "hide" } else { "show" });
            }
            Step::Engine(p, e) => {
                let Some(place) = self.places.get(p) else { println!("step: engine — no place {p}"); return };
                place.obj.set_engine(if e == 4 { zxr_managed_place_v1::Engine::Custom } else { zxr_managed_place_v1::Engine::Free });
                println!("step: set_engine place {p} -> {}", if e == 4 { "custom" } else { "free" });
            }
            Step::Wait(_) | Step::Hold | Step::Stall => {}
        }
    }

    fn print_picture(&self) {
        println!("picture: capabilities={} limits={:?} sequences={}", self.capabilities, self.limits, self.sequences);
        for (i, p) in self.places.iter().enumerate() {
            println!("picture: place {i} engine={} members={:?}", p.engine, p.members);
        }
        for (i, w) in self.windows.iter().enumerate() {
            println!(
                "picture: window {i} app_id={:?} title={:?} place={} pos=({:.2},{:.2},{:.2}) closed={}",
                w.app_id,
                w.title,
                w.place.map(|p| p.to_string()).unwrap_or_else(|| "none".into()),
                w.pos[0],
                w.pos[1],
                w.pos[2],
                w.closed
            );
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(state: &mut State, registry: &WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<State>) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            if interface == ZxrWindowManagerV1::interface().name && state.manager.is_none() {
                let m = registry.bind::<ZxrWindowManagerV1, _, _>(name, version.min(1), qh, ());
                println!("bound: {interface} v{version}");
                state.manager = Some(m);
            }
        }
    }
}

impl Dispatch<ZxrWindowManagerV1, ()> for State {
    fn event(state: &mut State, manager: &ZxrWindowManagerV1, event: zxr_window_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        use zxr_window_manager_v1::Event as E;
        match event {
            E::Unavailable => {
                println!("ev: unavailable");
                state.unavailable = true;
            }
            E::Capabilities { capabilities } => {
                let c = match capabilities {
                    WEnum::Value(v) => v.bits(),
                    WEnum::Unknown(v) => v,
                };
                println!("ev: capabilities {c}");
                state.capabilities = c;
            }
            E::Limits { min_distance, max_distance, max_angular_size } => {
                println!("ev: limits min={min_distance} max={max_distance} angular={max_angular_size}");
                state.limits = Some((min_distance, max_distance, max_angular_size));
            }
            E::Finished => {
                println!("ev: finished");
                state.finished = true;
            }
            E::ManageStart { serial } => {
                state.sequences += 1;
                println!("ev: manage_start {serial}");
                if state.stalled {
                    return;
                }
                if state.run_steps(true) {
                    manager.manage_finish(serial);
                }
            }
            E::RenderStart { serial } => {
                println!("ev: render_start {serial}");
                if state.stalled {
                    return;
                }
                if state.run_steps(false) {
                    manager.render_finish(serial);
                    // steps exhausted (and not holding) → the picture, then leave; steps left
                    // that are ours to drive → ask for the sequence to carry them (`manage_dirty`)
                    match state.steps.front() {
                        None => state.done = true,
                        Some(Step::Hold) | Some(Step::Stall) | Some(Step::Focus(_, true)) => {}
                        Some(_) => manager.manage_dirty(),
                    }
                }
            }
            E::Window { id } => {
                println!("ev: window {}", state.windows.len());
                state.windows.push(Window { obj: id, app_id: String::new(), title: String::new(), place: None, pos: [0.0; 3], closed: false });
            }
            E::Place { id } => {
                println!("ev: place {}", state.places.len());
                state.places.push(Place { obj: id, engine: 0, members: Vec::new() });
            }
        }
    }

    wayland_client::event_created_child!(State, ZxrWindowManagerV1, [
        zxr_window_manager_v1::EVT_WINDOW_OPCODE => (ZxrManagedWindowV1, ()),
        zxr_window_manager_v1::EVT_PLACE_OPCODE => (ZxrManagedPlaceV1, ()),
    ]);
}

impl Dispatch<ZxrManagedWindowV1, ()> for State {
    fn event(state: &mut State, w: &ZxrManagedWindowV1, event: zxr_managed_window_v1::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        use zxr_managed_window_v1::Event as E;
        let Some(i) = state.window_index(w) else { return };
        match event {
            E::Kind { kind } => println!("ev: window {i} kind={:?}", kind),
            E::AppId { app_id } => {
                println!("ev: window {i} app_id={app_id:?}");
                state.windows[i].app_id = app_id;
            }
            E::Title { title } => {
                println!("ev: window {i} title={title:?}");
                state.windows[i].title = title;
            }
            E::Parent { parent } => println!("ev: window {i} parent={:?}", parent.and_then(|p| state.window_index(&p))),
            E::DimensionsHint { min_width, min_height, max_width, max_height } => println!("ev: window {i} hint=[{min_width}x{min_height}..{max_width}x{max_height}]"),
            E::Dimensions { width, height } => println!("ev: window {i} dimensions={width}x{height}"),
            E::State { place, pose, scale_mode } => {
                let p = place.and_then(|p| state.place_index(&p));
                let t = translation(&pose);
                println!("ev: window {i} state place={} pos=({:.2},{:.2},{:.2}) scale={:?}", p.map(|p| p.to_string()).unwrap_or_else(|| "none".into()), t[0], t[1], t[2], scale_mode);
                state.windows[i].place = p;
                state.windows[i].pos = t;
            }
            E::MoveRequested { serial } => println!("ev: window {i} move_requested serial={serial}"),
            E::ResizeRequested { serial, edges } => println!("ev: window {i} resize_requested serial={serial} edges={:?}", edges),
            E::MaximizeRequested => println!("ev: window {i} maximize_requested"),
            E::UnmaximizeRequested => println!("ev: window {i} unmaximize_requested"),
            E::FullscreenRequested => println!("ev: window {i} fullscreen_requested"),
            E::ExitFullscreenRequested => println!("ev: window {i} exit_fullscreen_requested"),
            E::MinimizeRequested => println!("ev: window {i} minimize_requested"),
            E::ExclusiveRequested => println!("ev: window {i} exclusive_requested"),
            E::Closed => {
                println!("ev: window {i} closed");
                state.windows[i].closed = true;
            }
            E::Interaction { serial } => {
                println!("ev: window {i} interaction serial={serial}");
                state.last_interaction = Some(serial);
                if let Some(m) = state.manager.as_ref() {
                    // a fresh-focus step waiting on this: ask for a sequence to carry it
                    if matches!(state.steps.front(), Some(Step::Focus(_, true))) {
                        m.manage_dirty();
                    }
                }
            }
        }
    }
}

impl Dispatch<ZxrManagedPlaceV1, ()> for State {
    fn event(state: &mut State, p: &ZxrManagedPlaceV1, event: zxr_managed_place_v1::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        use zxr_managed_place_v1::Event as E;
        let Some(i) = state.place_index(p) else { return };
        match event {
            E::Engine { engine } => {
                let e = match engine {
                    WEnum::Value(v) => v as u32,
                    WEnum::Unknown(v) => v,
                };
                println!("ev: place {i} engine={e}");
                state.places[i].engine = e;
            }
            E::MemberEnter { window } => {
                if let Some(w) = state.window_index(&window) {
                    println!("ev: place {i} member_enter window {w}");
                    if !state.places[i].members.contains(&w) {
                        state.places[i].members.push(w);
                    }
                }
            }
            E::MemberLeave { window } => {
                if let Some(w) = state.window_index(&window) {
                    println!("ev: place {i} member_leave window {w}");
                    state.places[i].members.retain(|x| *x != w);
                }
            }
            E::Done => println!("ev: place {i} done"),
            E::Removed => println!("ev: place {i} removed"),
        }
    }
}

fn main() {
    let mut state = State::default();
    for a in std::env::args().skip(1) {
        match Step::parse(&a) {
            Some(s) => state.steps.push_back(s),
            None => {
                eprintln!("unknown step {a:?}");
                exit(64);
            }
        }
    }
    let conn = Connection::connect_to_env().unwrap_or_else(|e| {
        eprintln!("connect: {e}");
        exit(3)
    });
    let display = conn.display();
    let mut queue: EventQueue<State> = conn.new_event_queue();
    let qh = queue.handle();
    let _registry = display.get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");
    if state.manager.is_none() {
        println!("no zxr_window_manager_v1 global");
        exit(3);
    }
    loop {
        if let Err(e) = queue.blocking_dispatch(&mut state) {
            match conn.protocol_error() {
                Some(pe) => {
                    println!("protocol error: object {} code {} — {}", pe.object_id, pe.code, pe.message);
                    exit(4);
                }
                None => {
                    println!("connection lost: {e}");
                    exit(4);
                }
            }
        }
        if state.unavailable {
            println!("exit: unavailable");
            exit(2);
        }
        if state.finished {
            println!("exit: finished");
            state.print_picture();
            exit(0);
        }
        if state.done {
            state.print_picture();
            // a clean leave: `destroy` (the disconnect contract is the same as death)
            if let Some(m) = state.manager.take() {
                m.destroy();
            }
            if queue.roundtrip(&mut state).is_err() {
                if let Some(pe) = conn.protocol_error() {
                    println!("protocol error: object {} code {} — {}", pe.object_id, pe.code, pe.message);
                    exit(4);
                }
            }
            exit(0);
        }
    }
}
