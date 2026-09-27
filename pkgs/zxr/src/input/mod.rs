//! The `input` module (specs/zxr-core.md §8; docs/architecture/spatial-input.md §1a, ruled
//! 2026-09-26; research/68).
//!
//! **Where it lives:** in the compositor, on the state loop. libinput and EI arrive as calloop
//! sources whenever a device speaks; the XR sources arrive once per tick with `xrSyncActions`,
//! after `xrLocateViews` and before the flatten. Both feed the same ordered stages.
//!
//! **The two seams** (research/68 §5): sources enter through the OpenXR action set (XR devices —
//! a new controller is a bindings entry, not code) and smithay's `InputBackend` (libinput, EI);
//! inside zxr every source is one of a **closed set of kinds** ([`SourceKind`]) and every event
//! is one [`Sample`], so the stages are kind-agnostic and M2's forwarding to 3D clients is a copy.
//!
//! **The stages** ([`Slot`], in order — KWin's `InputFilterOrder` with the XR stages inserted
//! where their inputs exist, `references/kwin/src/input.h:366-393`): reserved system input →
//! mode (greeter/lock) → a11y transforms → stabilize → tier arbiter → hit test → WM grabs → IM →
//! seat. A stage returning [`Flow::Consumed`] stops the chain (`input.h:397-411`); the list is a
//! fixed array — nothing is registered or reordered at runtime (research/68 §9.2, ruled).
//!
//! This file is the **spine**: the types every stage shares, the chain, the intake queue, the
//! per-tick entry point, and no-op stages in every slot. The stages themselves live in sibling
//! files, one per slot or concern, and replace the no-ops in `Chain::default()` as they land.
//! `scene` names no Wayland or Vulkan type; this module names them only through [`Zxr`].

#![allow(dead_code)]

use openxr as xr;

use crate::state::Zxr;

// ---------------------------------------------------------------------------------------------
// Kinds, classes, flags
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    Left,
    Right,
}

/// The closed set of source kinds (spatial-input §1a; research/68 §9.2 ruled). The tier rule
/// (§3) is a `match` over this. Adding a kind the hardware contract does not name is an enum
/// variant plus an action binding — never a plugin.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SourceKind {
    /// the head ray — the floor (research/42); `hmdButtons` roles arrive on this kind
    Head,
    /// `XR_EXT_eye_gaze_interaction` gaze pose
    Gaze,
    /// `XR_EXT_hand_interaction` aim/pinch/poke/grasp per hand (or the joint bridge, §10)
    Hand(Side),
    /// a tracked controller's interaction profile
    Controller(Side),
    /// libinput / EI pointer devices: mouse, trackpad (relative motion, buttons, axes)
    Pointer,
    /// libinput / EI / virtual keyboards
    Keyboard,
}

impl SourceKind {
    pub const ALL: [SourceKind; 8] = [
        SourceKind::Head,
        SourceKind::Gaze,
        SourceKind::Hand(Side::Left),
        SourceKind::Hand(Side::Right),
        SourceKind::Controller(Side::Left),
        SourceKind::Controller(Side::Right),
        SourceKind::Pointer,
        SourceKind::Keyboard,
    ];

    /// The transport class a kind uses when it *targets* (§5, ruled): hands and gaze are
    /// touch-class; mice, trackpads and controllers are pointer-class; the head ray is
    /// pointer-class for its reticle but commits as touch (§8: "any device may commit").
    pub fn class(self) -> Class {
        match self {
            SourceKind::Gaze | SourceKind::Hand(_) => Class::Touch,
            SourceKind::Controller(_) | SourceKind::Pointer | SourceKind::Head => Class::Pointer,
            SourceKind::Keyboard => Class::Pointer,
        }
    }

    /// Whether this kind can be the *targeting* source of the tier rule (§3).
    pub fn targets(self) -> bool {
        !matches!(self, SourceKind::Keyboard)
    }

    pub fn parse(s: &str) -> Option<SourceKind> {
        Some(match s {
            "head" => SourceKind::Head,
            "gaze" => SourceKind::Gaze,
            "hand-left" | "hand_left" | "lhand" => SourceKind::Hand(Side::Left),
            "hand-right" | "hand_right" | "rhand" => SourceKind::Hand(Side::Right),
            "controller-left" | "controller_left" | "lctrl" => SourceKind::Controller(Side::Left),
            "controller-right" | "controller_right" | "rctrl" => SourceKind::Controller(Side::Right),
            "pointer" | "mouse" => SourceKind::Pointer,
            "keyboard" | "kbd" => SourceKind::Keyboard,
            _ => return None,
        })
    }
}

/// The two transports (spatial-input §5, ruled).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// `wl_touch`: a position only at `down`, each hand a contact, no hover
    Touch,
    /// `wl_pointer`: hover, cursor, axis; one logical pointer per seat
    Pointer,
}

/// Gaze quality per the extension's rule (`ext_eye_gaze_interaction.adoc:140-158`): only a
/// nominal pose targets; sub-nominal is never used for targeting; lost/unavailable degrade the
/// tier after the eyes→head timeout (§3). Other kinds report `Nominal` when tracked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    Nominal,
    SubNominal,
    #[default]
    Lost,
    Unavailable,
}

/// Bit flags on a sample (`FB_hand_tracking_aim`'s shape for the gesture bits, `xr.xml:8374-8376`).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Flags(pub u16);

impl Flags {
    /// the reserved system gesture is in progress on this hand (runtime flag or the bridge's)
    pub const SYSTEM_GESTURE: Flags = Flags(1 << 0);
    /// the system menu gesture completed (non-dominant hand, one frame)
    pub const MENU_PRESSED: Flags = Flags(1 << 1);
    /// this hand is the user's dominant hand
    pub const DOMINANT: Flags = Flags(1 << 2);
    /// produced by an EI client (emulated / remote) — libei's "distinction"
    pub const EMULATED: Flags = Flags(1 << 3);
    /// produced by the test injector (`zxr ctl source …`) — never set in production paths
    pub const SYNTHETIC: Flags = Flags(1 << 4);
    /// produced by the joint bridge rather than the runtime's interaction profile
    pub const BRIDGED: Flags = Flags(1 << 5);
    /// produced by an accessibility transform (dwell, mouse keys) rather than a device — KWin's
    /// dwell clicker gives its clicks a device of their own (`plugins/dwellclicker/dwellclicker.cpp:188-194`)
    pub const A11Y: Flags = Flags(1 << 6);
    /// the controller reports it is in the wearer's hand — the profile's `proximity`/`touch`
    /// component (`semantic_paths.adoc:551-554, 578-588`); overrides the held heuristic (`held.rs`)
    pub const IN_HAND: Flags = Flags(1 << 7);

    pub fn contains(self, f: Flags) -> bool {
        self.0 & f.0 == f.0
    }
    pub fn insert(&mut self, f: Flags) {
        self.0 |= f.0;
    }
    pub fn remove(&mut self, f: Flags) {
        self.0 &= !f.0;
    }
}

/// Which physical control a button sample names (the action-set vocabulary, spatial-input §2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Button {
    /// primary: controller `select`/trigger, `hmdButtons.selectRole`, mouse `BTN_LEFT`
    Select,
    /// the profile's secondary / `BTN_RIGHT`
    Secondary,
    /// `BTN_MIDDLE` or a third profile button
    Middle,
    /// controller `menu`
    Menu,
    /// `hmdButtons.backRole`
    Back,
    /// the reserved `system` control (never forwarded — the reserved stage consumes it)
    System,
    /// the profile's grasp/grip click
    Grip,
    /// any other evdev button code
    Code(u32),
}

/// Where an axis sample came from — maps onto `wl_pointer.axis_source`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxisSource {
    Wheel,
    Finger,
    Continuous,
}

/// The `XR_EXT_hand_interaction` values (`ext_hand_interaction.adoc:298-338`) and their
/// controller analogues, 0..1.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Values {
    pub pinch: f32,
    pub aim_activate: f32,
    pub grasp: f32,
    /// poke depth along the plane normal, metres (negative = behind the plane)
    pub poke: f32,
}

/// One input event or per-tick state from one source (spatial-input §1a). One struct for every
/// kind — a few fields are unused per kind — so the stages are kind-agnostic and §11's forwarding
/// to 3D clients is a copy (plan judgment 4).
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub kind: SourceKind,
    /// CLOCK_MONOTONIC ns of the event (libinput/EI) or of the tick that read it (XR)
    pub time_ns: u64,
    /// the predicted display time the XR state was synced for, when it came from the runtime
    pub xr_time: Option<xr::Time>,
    /// aim / gaze / head pose in LOCAL; `None` for pure device events (a key, a wheel tick)
    pub pose: Option<xr::Posef>,
    /// the poke pose for hands (LOCAL), when the profile provides one
    pub poke_pose: Option<xr::Posef>,
    /// the pose's tracked bits (both set) — nominal for gaze
    pub tracked: bool,
    /// `ready_ext`: the hand/controller is in a state to act (spatial-input §2)
    pub ready: bool,
    pub quality: Quality,
    /// relative pointer motion in device units (libinput/EI), if any
    pub delta: Option<(f64, f64)>,
    /// scroll: (horizontal, vertical) in the source's units, with `axis_source`
    pub axis: Option<(f64, f64)>,
    pub axis_source: Option<AxisSource>,
    /// a button edge, if this sample is one
    pub button: Option<(Button, bool)>,
    /// a key edge (evdev code, pressed), if this sample is one
    pub key: Option<(u32, bool)>,
    /// continuous values (pinch, grasp, …) for XR kinds
    pub values: Values,
    pub flags: Flags,
}

impl Sample {
    pub fn new(kind: SourceKind, time_ns: u64) -> Sample {
        Sample {
            kind,
            time_ns,
            xr_time: None,
            pose: None,
            poke_pose: None,
            tracked: false,
            ready: false,
            quality: Quality::Lost,
            delta: None,
            axis: None,
            axis_source: None,
            button: None,
            key: None,
            values: Values::default(),
            flags: Flags::default(),
        }
    }

    pub fn with_pose(mut self, pose: xr::Posef) -> Sample {
        self.pose = Some(pose);
        self.tracked = true;
        self.ready = true;
        self.quality = Quality::Nominal;
        self
    }

    pub fn with_button(mut self, b: Button, pressed: bool) -> Sample {
        self.button = Some((b, pressed));
        self
    }

    pub fn is_event(&self) -> bool {
        self.button.is_some() || self.key.is_some() || self.delta.is_some() || self.axis.is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------------------------

/// What a stage did with a sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// pass it to the next stage
    Continue,
    /// stop: no later stage sees this sample (KWin: a filter returning `true`)
    Consumed,
}

/// One stage of the pipeline. `run` sees every sample in order; `tick` runs once per frame
/// after all samples (timers: dwell, hold detection, tier timeouts, the reserved press map).
pub trait Stage {
    fn name(&self) -> &'static str;
    fn run(&mut self, sample: &mut Sample, st: &mut Zxr) -> Flow;
    fn tick(&mut self, _st: &mut Zxr, _time_ns: u64) {}
}

/// The slots, in the ruled order (spatial-input §1a). The index *is* the order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Slot {
    /// the reserved system input: consumed here, never forwarded (native-openxr-apps §6)
    Reserved = 0,
    /// `--greeter` / lock: only the auth scene / exclusive layer-shell may receive below
    Mode = 1,
    /// dwell-as-commit, sticky/slow keys, pointer gain — transforms on raw events
    A11y = 2,
    /// per-source filter, target lock, relaxation, event-time compensation (§4)
    Stabilize = 3,
    /// the arbiter: which kind targets now; class = touch or pointer (§3, §5)
    Tier = 4,
    /// scene member pass → plane-local point → surface tree (spec §5a)
    Hit = 5,
    /// WM policy: move/resize/grab-all, popup grab, affordances, DnD
    Grabs = 6,
    /// text-input / input-method routing — sees only what nothing above consumed
    Im = 7,
    /// `wl_touch` / `wl_pointer` / `wl_keyboard` emission; xdg-activation; cursors (§7)
    Seat = 8,
}

pub const SLOT_COUNT: usize = 9;

impl Slot {
    pub const ALL: [Slot; SLOT_COUNT] = [Slot::Reserved, Slot::Mode, Slot::A11y, Slot::Stabilize, Slot::Tier, Slot::Hit, Slot::Grabs, Slot::Im, Slot::Seat];
}

/// A slot with nothing in it yet.
pub struct NoOp(pub Slot);

impl Stage for NoOp {
    fn name(&self) -> &'static str {
        match self.0 {
            Slot::Reserved => "noop:reserved",
            Slot::Mode => "noop:mode",
            Slot::A11y => "noop:a11y",
            Slot::Stabilize => "noop:stabilize",
            Slot::Tier => "noop:tier",
            Slot::Hit => "noop:hit",
            Slot::Grabs => "noop:grabs",
            Slot::Im => "noop:im",
            Slot::Seat => "noop:seat",
        }
    }
    fn run(&mut self, _s: &mut Sample, _st: &mut Zxr) -> Flow {
        Flow::Continue
    }
}

/// The fixed, ordered stage list. `set` fills a slot; there is no `insert`, `append` or
/// reorder — the order is the design's, not the caller's.
pub struct Chain {
    stages: [Box<dyn Stage>; SLOT_COUNT],
    /// per-slot count of samples consumed there (journal)
    pub consumed: [u64; SLOT_COUNT],
    pub samples: u64,
}

impl Default for Chain {
    fn default() -> Self {
        Chain {
            stages: [
                Box::new(NoOp(Slot::Reserved)),
                Box::new(NoOp(Slot::Mode)),
                Box::new(NoOp(Slot::A11y)),
                Box::new(NoOp(Slot::Stabilize)),
                Box::new(NoOp(Slot::Tier)),
                Box::new(NoOp(Slot::Hit)),
                Box::new(NoOp(Slot::Grabs)),
                Box::new(NoOp(Slot::Im)),
                Box::new(NoOp(Slot::Seat)),
            ],
            consumed: [0; SLOT_COUNT],
            samples: 0,
        }
    }
}

impl Chain {
    pub fn set(&mut self, slot: Slot, stage: Box<dyn Stage>) {
        self.stages[slot as usize] = stage;
    }

    pub fn get(&self, slot: Slot) -> &dyn Stage {
        self.stages[slot as usize].as_ref()
    }

    pub fn get_mut(&mut self, slot: Slot) -> &mut dyn Stage {
        self.stages[slot as usize].as_mut()
    }

    /// Run one sample through the stages in order; returns the slot that consumed it, if any.
    pub fn run(&mut self, sample: &mut Sample, st: &mut Zxr) -> Option<Slot> {
        self.samples += 1;
        for (i, stage) in self.stages.iter_mut().enumerate() {
            if stage.run(sample, st) == Flow::Consumed {
                self.consumed[i] += 1;
                return Some(Slot::ALL[i]);
            }
        }
        None
    }

    /// The per-tick hook of every stage, in order.
    pub fn tick(&mut self, st: &mut Zxr, time_ns: u64) {
        for stage in self.stages.iter_mut() {
            stage.tick(st, time_ns);
        }
    }

    pub fn names(&self) -> [&'static str; SLOT_COUNT] {
        let mut out = [""; SLOT_COUNT];
        for (i, s) in self.stages.iter().enumerate() {
            out[i] = s.name();
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// The module state and the per-tick entry point
// ---------------------------------------------------------------------------------------------

/// The compositor's input mode (ADR 0007): the mode stage gates everything below it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// `--greeter`: only the auth scene / exclusive layer-shell may receive
    Greeter,
    /// the built-in lock (ADR 0007 I1–I3)
    Locked,
}

/// The tier arbiter's output (spatial-input §3): which kind targets now, with which class, and
/// whether it is direct touch. Written by the Tier stage, read by everything after it.
pub use tier::Selection;

/// One hit of a targeting ray this tick (spec §5a hit test): written by the Hit stage for the
/// sample's kind, read by the transport stages. Plane-local metres, y up; `distance` along the
/// ray. The surface under the point is resolved by the transport through `Zxr::hit_surface_at`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hit {
    pub kind: SourceKind,
    pub member: crate::scene::MemberId,
    pub local: [f32; 2],
    pub distance: f32,
    pub time_ns: u64,
}

/// The `input` module's state on `Zxr`.
pub struct Input {
    pub chain: Chain,
    /// the tier arbiter's current selection (`None` until the Tier stage has run once)
    pub tier: Option<Selection>,
    /// this tick's hits, one per targeting sample that hit a plane; cleared at every tick
    pub hits: Vec<Hit>,
    /// greeter / lock / normal — set by `--greeter`, the lock machine, the control socket
    pub mode: Mode,
    /// XR sources suspended (doff, docked): the mode stage sets it from `present`; peripherals
    /// continue (spatial-input §1a)
    pub xr_suspended: bool,
    /// samples produced between ticks (libinput, EI, the injector) and by the XR sync; drained
    /// in order at the tick
    pub queue: Vec<Sample>,
    /// user presence from the runtime (`XR_EXT_user_presence`) or the injector; `None` = unknown
    pub present: Option<bool>,
    /// set by whoever changes `present`; the mode stage consumes it at the next tick
    pub presence_changed: bool,
    /// the head pose this tick (the `Views` frame), for the floor and for stages that need it
    pub head: Option<xr::Posef>,
    /// the oldest event sample processed this tick (for the event → `xrEndFrame` latency the
    /// input gate measures; `main.rs` closes it after `xrEndFrame`)
    pub tick_oldest_event_ns: Option<u64>,
    /// this tick's event count and timestamp sum, for the per-event mean (the oldest-event
    /// number above is one display period by construction under a saturating stream)
    pub tick_event_count: u64,
    pub tick_event_time_sum_ns: u128,
    /// the test-only injector's latched per-kind state
    pub injector: Injector,
    /// last-input record for the idle ladder (ADR 0007) — a side effect of every stage, never a
    /// slot: KWin's `UserActivitySpy` shape (`references/kwin/src/input.cpp:3169-3172`)
    pub activity: activity::Activity,
    /// a11y settings pushed from the control socket (later `org.mura.Settings1`, spatial-input
    /// §14); the A11y stage takes them at its next `tick`
    pub a11y_dwell: Option<bool>,
    pub a11y_gain: Option<f64>,
    /// cursor preferences pushed from the control socket (later `org.mura.Settings1`
    /// `input.cursor.ray` / `input.cursor.scale`, spatial-input §14); the seat stage takes them
    pub cursor_ray: Option<cursor::RayCursor>,
    pub cursor_scale: Option<cursor::Scale>,
    /// the client's cursor as `SeatHandler::cursor_image` last reported it (cursor-shape names and
    /// `set_cursor` surfaces alike); taken by the seat stage each tick (spatial-input §7)
    pub cursor_image: Option<smithay::input::pointer::CursorImageStatus>,
    /// this tick's one cursor layer (§7, research/70 §9): the ring, the client's image, or both,
    /// for the frame procedure's one band-5 quad; `None` = nothing submitted
    pub cursor_layer: Option<cursor::CursorLayer>,
    /// the inputs the layer was resolved from (`zxr ctl list` diagnostics)
    pub cursor_inputs: Option<cursor::Inputs>,
    /// ticks a `cursor-shape-v1` name was current and the theme had no image for it
    pub cursor_named_ticks: u64,
    /// the touch-class emphasis target this tick (member, level ∈ [0,1]) — spatial-input §4
    pub emphasis: Option<(crate::scene::MemberId, f32)>,
}

impl Default for Input {
    fn default() -> Self {
        Input { chain: Chain::default(), tier: None, hits: Vec::with_capacity(8), mode: Mode::default(), xr_suspended: false, queue: Vec::with_capacity(64), present: None, presence_changed: false, head: None, tick_oldest_event_ns: None, tick_event_count: 0, tick_event_time_sum_ns: 0, injector: Injector::default(), activity: activity::Activity::default(), a11y_dwell: None, a11y_gain: None, cursor_ray: None, cursor_scale: None, cursor_image: None, cursor_layer: None, cursor_inputs: None, cursor_named_ticks: 0, emphasis: None }
    }
}

impl Input {
    /// Queue a sample from an event source (libinput, EI, the injector).
    pub fn push(&mut self, s: Sample) {
        self.queue.push(s);
    }

    /// Record a presence change (the runtime's event or the injector's `present on|off`).
    pub fn set_present(&mut self, present: bool) {
        if self.present != Some(present) {
            self.present = Some(present);
            self.presence_changed = true;
        }
    }
}

/// Run one event sample through the chain **now** — the per-event dispatch of the state-loop
/// shape (research/68 §9.1, ruled: libinput/EI events are dispatched when they arrive, not
/// batched to the tick; the tick-bound read is the recorded rethink candidate, not the default).
/// XR samples still arrive at the tick (`tick`), since that is when the runtime has them. If no
/// tick has run yet (no head pose), the sample is queued for the first one.
pub fn dispatch(st: &mut Zxr, mut s: Sample, now_ns: u64) {
    if st.input.head.is_none() {
        st.input.queue.push(s);
        return;
    }
    if s.is_event() && s.time_ns <= now_ns && s.time_ns > 0 {
        let age = now_ns - s.time_ns;
        st.journal.input_event_age_ns_total += age;
        st.journal.input_event_age_ns_max = st.journal.input_event_age_ns_max.max(age);
        st.journal.input_events += 1;
        st.input.tick_oldest_event_ns = Some(st.input.tick_oldest_event_ns.map_or(s.time_ns, |t| t.min(s.time_ns)));
        st.input.tick_event_count += 1;
        st.input.tick_event_time_sum_ns += s.time_ns as u128;
    }
    let mut chain = std::mem::take(&mut st.input.chain);
    if let Some(slot) = chain.run(&mut s, st) {
        st.journal.input_consumed[slot as usize] += 1;
    }
    st.journal.input_samples += 1;
    st.input.chain = chain;
    // anything the stages pushed (a cancel, a dwell commit) follows immediately, in order
    if !st.input.queue.is_empty() {
        let mut q = std::mem::take(&mut st.input.queue);
        for s2 in q.drain(..) {
            dispatch(st, s2, now_ns);
        }
    }
}

/// The per-tick entry point (spec §7: after `xrLocateViews`, before the flatten). Produces the
/// head sample from the views, lets the XR side queue its action samples, drains the queue
/// through the chain, then runs every stage's `tick` hook.
pub fn tick(st: &mut Zxr, head: xr::Posef, time: xr::Time, now_ns: u64) {
    st.input.head = Some(head);
    st.input.hits.clear();
    // the floor: the head ray is always a sample (research/42), per tick
    let mut head_sample = Sample::new(SourceKind::Head, now_ns).with_pose(head);
    head_sample.xr_time = Some(time);
    st.input.queue.insert(0, head_sample);
    // XR action samples (lane D fills `XrCore::sync_samples`; a no-op until then)
    st.xr.sync_samples(time, now_ns, &mut st.input.queue);

    let mut chain = std::mem::take(&mut st.input.chain);
    let mut queue = std::mem::take(&mut st.input.queue);
    for mut s in queue.drain(..) {
        // intake latency: from the event's timestamp (libinput/EI/injector) to the tick that
        // processes it — the input gate's number, with `wake_to_end` covering the rest of the
        // path to `xrEndFrame` (research/68 §9.1 trigger)
        if s.is_event() && s.time_ns <= now_ns && s.time_ns > 0 {
            let age = now_ns - s.time_ns;
            st.journal.input_event_age_ns_total += age;
            st.journal.input_event_age_ns_max = st.journal.input_event_age_ns_max.max(age);
            st.journal.input_events += 1;
            st.input.tick_oldest_event_ns = Some(st.input.tick_oldest_event_ns.map_or(s.time_ns, |t| t.min(s.time_ns)));
            st.input.tick_event_count += 1;
            st.input.tick_event_time_sum_ns += s.time_ns as u128;
        }
        if let Some(slot) = chain.run(&mut s, st) {
            st.journal.input_consumed[slot as usize] += 1;
        }
        st.journal.input_samples += 1;
    }
    chain.tick(st, now_ns);
    // put the (possibly refilled) queue back without losing what stages pushed meanwhile
    queue.append(&mut st.input.queue);
    st.input.queue = queue;
    st.input.chain = chain;
}

// ---------------------------------------------------------------------------------------------
// The injector (test-only): `zxr ctl SOCKET source <kind> …` → a synthetic sample in the queue
// ---------------------------------------------------------------------------------------------

/// Per-kind latched state for the injector, so a `press` after a `pose` carries the pose and a
/// `value` keeps the last pose/buttons (the runtime's action state is level-triggered too).
#[derive(Default)]
pub struct Injector {
    latched: Vec<(SourceKind, Sample)>,
    /// the joint bridge's per-hand state for `source <hand> joints …` (hysteresis, hold)
    bridge: [bridge::State; 2],
    /// the bridge's configuration — the settings' (settings.rs `apply`), so the harness's joints
    /// are judged as the runtime's would be
    pub bridge_cfg: bridge::BridgeCfg,
}

impl Injector {
    fn latch(&mut self, kind: SourceKind, now_ns: u64) -> &mut Sample {
        if let Some(i) = self.latched.iter().position(|(k, _)| *k == kind) {
            let s = &mut self.latched[i].1;
            s.time_ns = now_ns;
            s.button = None;
            s.key = None;
            s.delta = None;
            s.axis = None;
            s.axis_source = None;
            return s;
        }
        let mut s = Sample::new(kind, now_ns);
        s.flags.insert(Flags::SYNTHETIC);
        self.latched.push((kind, s));
        &mut self.latched.last_mut().unwrap().1
    }

    /// Translate one injector command into a queued sample. Returns the reply line.
    pub fn apply(&mut self, cmd: &crate::control::SourceCmd, now_ns: u64, head: Option<xr::Posef>, queue: &mut Vec<Sample>) -> Result<String, String> {
        use crate::control::SourceCmd as C;
        let kind_of = |k: &str| SourceKind::parse(k).ok_or_else(|| format!("unknown source kind {k}"));
        let reply;
        let s: Sample = match cmd {
            C::Pose { kind, pos, quat, quality } => {
                let k = kind_of(kind)?;
                let s = self.latch(k, now_ns);
                s.pose = Some(xr::Posef { orientation: xr::Quaternionf { x: quat[0], y: quat[1], z: quat[2], w: quat[3] }, position: xr::Vector3f { x: pos[0], y: pos[1], z: pos[2] } });
                s.quality = match quality.as_str() {
                    "nominal" => Quality::Nominal,
                    "subnominal" | "sub-nominal" => Quality::SubNominal,
                    "lost" => Quality::Lost,
                    "unavailable" => Quality::Unavailable,
                    other => return Err(format!("unknown quality {other}")),
                };
                s.tracked = s.quality == Quality::Nominal;
                s.ready = s.quality != Quality::Unavailable && s.quality != Quality::Lost;
                reply = format!("{k:?} pose {quality}");
                *s
            }
            C::Button { kind, button, pressed } => {
                let k = kind_of(kind)?;
                let b = match button.as_str() {
                    "select" => Button::Select,
                    "secondary" => Button::Secondary,
                    "middle" => Button::Middle,
                    "menu" => Button::Menu,
                    "back" => Button::Back,
                    "system" => Button::System,
                    "grip" => Button::Grip,
                    other => Button::Code(other.parse().map_err(|_| format!("unknown button {other}"))?),
                };
                let s = self.latch(k, now_ns);
                s.button = Some((b, *pressed));
                reply = format!("{k:?} {b:?} {}", if *pressed { "press" } else { "release" });
                *s
            }
            C::Value { kind, name, value } => {
                let k = kind_of(kind)?;
                let s = self.latch(k, now_ns);
                match name.as_str() {
                    "pinch" => s.values.pinch = *value,
                    "aim_activate" => s.values.aim_activate = *value,
                    "grasp" => s.values.grasp = *value,
                    "poke" => s.values.poke = *value,
                    other => return Err(format!("unknown value {other}")),
                }
                reply = format!("{k:?} {name}={value}");
                *s
            }
            C::Delta { kind, dx, dy } => {
                let k = kind_of(kind)?;
                let s = self.latch(k, now_ns);
                s.delta = Some((*dx, *dy));
                reply = format!("{k:?} delta");
                *s
            }
            C::Axis { kind, h, v, source } => {
                let k = kind_of(kind)?;
                let s = self.latch(k, now_ns);
                s.axis = Some((*h, *v));
                s.axis_source = Some(match source.as_str() {
                    "wheel" => AxisSource::Wheel,
                    "finger" => AxisSource::Finger,
                    "continuous" => AxisSource::Continuous,
                    other => return Err(format!("unknown axis source {other}")),
                });
                reply = format!("{k:?} axis");
                *s
            }
            C::Flag { kind, name, on } => {
                let k = kind_of(kind)?;
                let f = match name.as_str() {
                    "system_gesture" => Flags::SYSTEM_GESTURE,
                    "menu_pressed" => Flags::MENU_PRESSED,
                    "dominant" => Flags::DOMINANT,
                    other => return Err(format!("unknown flag {other}")),
                };
                let s = self.latch(k, now_ns);
                if *on {
                    s.flags.insert(f)
                } else {
                    s.flags.remove(f)
                }
                reply = format!("{k:?} {name} {}", if *on { "on" } else { "off" });
                *s
            }
            C::Joints { kind, joints } => {
                let k = kind_of(kind)?;
                // the §10 joint bridge, driven by the harness (no simulated hands exist)
                let SourceKind::Hand(side) = k else { return Err(format!("joints need a hand kind, got {k:?}")) };
                let i = if side == Side::Left { 0 } else { 1 };
                let s = bridge::bridge_from_joints_cfg(&self.bridge_cfg, k, joints, head.unwrap_or(xr::Posef::IDENTITY), now_ns, &mut self.bridge[i]);
                *self.latch(k, now_ns) = s; // a later `press` carries the bridged aim
                reply = format!("{k:?} joints pinch={:.2} flags={:?}", s.values.pinch, s.flags);
                s
            }
            C::Off { kind } => {
                let k = kind_of(kind)?;
                let s = self.latch(k, now_ns);
                s.pose = None;
                s.tracked = false;
                s.ready = false;
                s.quality = Quality::Lost;
                reply = format!("{k:?} off");
                *s
            }
        };
        queue.push(s);
        Ok(reply)
    }
}

// ---------------------------------------------------------------------------------------------
// The floor stage: the R0 head-ray pointer, so behaviour is unchanged until the lanes land
// ---------------------------------------------------------------------------------------------

/// Spine stand-in in the `Seat` slot: a `Head` sample with a pose drives the gaze pointer exactly
/// as R0's `update_gaze_pointer` did. Replaced by the tier/hit/transport stages (lanes A–C).
pub struct HeadFloor;

impl Stage for HeadFloor {
    fn name(&self) -> &'static str {
        "seat:head-floor"
    }
    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        if s.kind == SourceKind::Head {
            if let Some(pose) = s.pose {
                st.update_gaze_pointer(pose);
            }
            return Flow::Consumed;
        }
        Flow::Continue
    }
}

// ---------------------------------------------------------------------------------------------
// Tests: chain order and short-circuit, no runtime
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_in_the_ruled_order() {
        let names: Vec<&str> = Chain::default().names().to_vec();
        assert_eq!(names, vec!["noop:reserved", "noop:mode", "noop:a11y", "noop:stabilize", "noop:tier", "noop:hit", "noop:grabs", "noop:im", "noop:seat"]);
        for (i, s) in Slot::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
    }

    #[test]
    fn kinds_classes_and_parse() {
        assert_eq!(SourceKind::Gaze.class(), Class::Touch);
        assert_eq!(SourceKind::Hand(Side::Left).class(), Class::Touch);
        assert_eq!(SourceKind::Controller(Side::Right).class(), Class::Pointer);
        assert_eq!(SourceKind::Pointer.class(), Class::Pointer);
        assert!(!SourceKind::Keyboard.targets());
        for k in SourceKind::ALL {
            assert!(k.targets() || k == SourceKind::Keyboard);
        }
        assert_eq!(SourceKind::parse("hand-left"), Some(SourceKind::Hand(Side::Left)));
        assert_eq!(SourceKind::parse("rctrl"), Some(SourceKind::Controller(Side::Right)));
        assert_eq!(SourceKind::parse("nope"), None);
    }

    #[test]
    fn flags_and_values() {
        let mut f = Flags::default();
        f.insert(Flags::SYSTEM_GESTURE);
        f.insert(Flags::SYNTHETIC);
        assert!(f.contains(Flags::SYSTEM_GESTURE) && f.contains(Flags::SYNTHETIC) && !f.contains(Flags::EMULATED));
        f.remove(Flags::SYNTHETIC);
        assert!(!f.contains(Flags::SYNTHETIC));
        let s = Sample::new(SourceKind::Pointer, 1).with_button(Button::Select, true);
        assert!(s.is_event());
        assert!(!Sample::new(SourceKind::Head, 1).is_event());
    }

    // A chain test without a `Zxr`: the order and short-circuit are properties of `Chain`'s
    // iteration, exercised here through a recording stage set that never touches `st`.
    // `Chain::run` needs a `&mut Zxr`, which cannot be built without a runtime, so the
    // iteration rule is tested on a mirror with the same body.
    struct Rec(Vec<usize>, usize, Option<usize>);
    fn mirror_run(order: &mut Rec) -> Option<usize> {
        for i in 0..SLOT_COUNT {
            order.0.push(i);
            if Some(i) == order.2 {
                return Some(i);
            }
        }
        None
    }

    #[test]
    fn consumed_short_circuits_and_order_is_fixed() {
        let mut r = Rec(Vec::new(), 0, Some(Slot::Mode as usize));
        assert_eq!(mirror_run(&mut r), Some(1));
        assert_eq!(r.0, vec![0, 1]);
        let mut r = Rec(Vec::new(), 0, None);
        assert_eq!(mirror_run(&mut r), None);
        assert_eq!(r.0, (0..SLOT_COUNT).collect::<Vec<_>>());
    }
}

pub mod stabilize;
pub mod hit;
// the stages above targeting: `Slot::Reserved`, `Slot::Mode` (+ presence), `Slot::A11y`, and the
// user-activity side effect (spatial-input §1a lines 113-117; ADR 0007; §13)
pub mod a11y;
pub mod activity;
pub mod mode;
pub mod reserved;
pub mod held;
pub mod loss;
pub mod quality;
pub mod tier;
pub mod actions;
pub mod bridge;
// lane C (spatial-input §5, §7, §8; ADR 0013 items 2, 3, 4, 6): the seat slot and its transports
pub mod cursor;
pub mod emphasis;
pub mod pointer;
pub mod seat;
pub mod touch;
// lane F: focus and activation, the text-entry seam, libinput and EI intake
pub mod focus;
pub mod text;
pub mod libinput;
pub mod ei;
pub mod theme;
