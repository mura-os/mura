//! The tier arbiter — which source kind targets now (spatial-input §3 "The tier rule (ruled)",
//! lines 190-213; §2's source table, lines 158-189; ADR 0013 amendment items 1, 2 and 4).
//!
//! **The rule.** Exactly one *targeting* source is active at a time; the *commit* device may be
//! any (§3 line 191, ADR item 1 — "gaze targets whatever commits — pinch, controller trigger, HMD
//! button, dwell"). Precedence, highest precision first:
//!
//! 1. **Gaze**, when nominal ([`quality`]) — tracked controllers do not take it over. Meta's
//!    opposite choice (with eyes on, controllers switch targeting to their ray) is the recorded
//!    dissent of research/63 §1 and §15 and is **not** adopted.
//! 2. **Controller aim ray**, when controllers are held ([`held`]) and gaze is absent or
//!    sub-nominal for longer than the fallback timeout. Pointer-class.
//! 3. **Hand aim ray**, when hands are tracked and neither of the above. Touch-class.
//! 4. **Head ray**, the floor (research/42 §4.4) — always available, so the ladder never empties.
//!
//! **Direct touch overrides a ray** for a hand inside the near band: enter at **0.18 m**, leave at
//! **0.22 m** — WiVRn's `palm_distance_close_thd_lo/hi`, "Distance to switch between touch and aim
//! interaction" (`references/wivrn/client/constants.h:45-47`, applied at
//! `client/render/imgui_impl.cpp:618-625`), the one comparable stating both bounds and the pair
//! research/36 §7 already selected for the keyboard.
//!
//! **Tier changes are events and never happen mid-gesture** (§3 lines 210-213: kwin-vr's "do not
//! stop movement when we already started", MRTK3's near-mode latch while a grab continues). While
//! any kind has a commit in progress the transition is *deferred*, not dropped, and it is applied
//! on release; [`loss`] guarantees the deferral cannot outlive the source that caused it.
//!
//! **Which side, when both are present**, is WiVRn's rule read off its source: if only one
//! candidate exists, it; otherwise a rising commit edge switches to that one; otherwise the last
//! one stays (`references/wivrn/client/render/imgui_impl.cpp:726-768`) — the same rule xrdesktop
//! states in prose: "when left clicking with a controller that is *not* used to do input synth,
//! make this controller do input synth" (`references/xrdesktop/src/xrd-input-synth.c:198-205`).
//!
//! **Budget** (overview invariant 9): fixed-size per-kind arrays, no allocation per sample, no
//! timer thread — the timeouts are evaluated from the sample's own `time_ns` and the stage's
//! per-tick hook.

use super::{held, loss, quality, Class, Flow, Quality, Sample, Side, SourceKind, Stage};
use crate::state::Zxr;

use loss::{kind_index, KINDS};

/// Every threshold and timeout the arbiter uses. All are stand-ins from a named comparable until
/// measured on Mura's trackers (spatial-input §15 item 1, the M1 gate).
#[derive(Clone, Copy, Debug)]
pub struct TierCfg {
    pub gaze: quality::GazeCfg,
    pub held: held::HeldCfg,
    /// direct touch is entered below this distance to the plane, metres
    /// (`references/wivrn/client/constants.h:45-46`)
    pub near_enter_m: f32,
    /// …and left above this one (`references/wivrn/client/constants.h:47`)
    pub near_leave_m: f32,
    /// a kind whose last sample is older than this is treated as absent. **Stand-in, flagged**:
    /// no comparable states one; set equal to the eyes→head fallback for symmetry. `Head` is
    /// exempt — `input::tick` pushes a head sample every tick, so the floor is never stale.
    pub stale_ns: u64,
}

impl Default for TierCfg {
    fn default() -> Self {
        let gaze = quality::GazeCfg::default();
        TierCfg { gaze, held: held::HeldCfg::default(), near_enter_m: 0.18, near_leave_m: 0.22, stale_ns: gaze.fallback_ns }
    }
}

/// The arbiter's answer, published for the stages below it (hit test, transports, cursors).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    /// the one kind that targets now
    pub targeting: SourceKind,
    /// its transport class (§5): touch for gaze and hands, pointer for controllers and the mouse
    pub class: Class,
    /// the targeting hand is inside the near band — direct touch, not a ray (§3 lines 203-208)
    pub direct: bool,
    /// when this selection was taken; a tier change is an event (§3 line 210) and this is its time
    pub changed_at_ns: u64,
}

/// The arbiter's published state. The orchestrator hangs one of these off `Input` at integration;
/// until then the stage owns it and `TierStage::current` reads it.
#[derive(Clone, Copy, Debug)]
pub struct TierState {
    pub targeting: SourceKind,
    pub class: Class,
    pub changed_at_ns: u64,
    /// direct-touch override in force (§3)
    pub direct: bool,
    /// the kind that *would* target if a commit were not in progress (§3 line 211); applied on
    /// release
    pub deferred: Option<SourceKind>,
    /// tier changes so far (journal: a tier change is an event, not a silent switch)
    pub changes: u64,
    /// transitions held back by a commit in progress
    pub deferrals: u64,
    /// sources that lost tracking mid-gesture and were cancelled ([`loss`])
    pub losses: u64,
    /// pending releases that never came back through the chain (should stay 0)
    pub expired: u64,
}

impl Default for TierState {
    fn default() -> Self {
        // the floor is the initial tier: the head ray is always available (§3 tier 4)
        TierState {
            targeting: SourceKind::Head,
            class: SourceKind::Head.class(),
            changed_at_ns: 0,
            direct: false,
            deferred: None,
            changes: 0,
            deferrals: 0,
            losses: 0,
            expired: 0,
        }
    }
}

impl TierState {
    pub fn current(&self) -> Selection {
        Selection { targeting: self.targeting, class: self.class, direct: self.direct, changed_at_ns: self.changed_at_ns }
    }
}

/// What one kind's last tracking report said.
#[derive(Clone, Copy, Debug, Default)]
struct Cap {
    seen: bool,
    tracked: bool,
    ready: bool,
    quality: Quality,
    last_ns: u64,
}

/// What [`Arbiter::observe`] produced.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// a synthetic release to queue: a source lost tracking mid-gesture ([`loss`]). The caller
    /// pushes it into `Input::queue`; the tier does not move until it has passed through.
    pub release: Option<Sample>,
    /// the tier changed on this sample — an event (§3 line 210)
    pub changed: bool,
    pub selection: Selection,
}

/// The tier rule, as a pure state machine over samples and a clock. `Stage::run` takes a
/// `&mut Zxr`, which no test can build, so every rule lives here and [`TierStage`] is a thin
/// adapter over it.
#[derive(Clone, Debug)]
pub struct Arbiter {
    pub cfg: TierCfg,
    /// the capability table: what is present and ready this tick, per kind (§3 line 192 — "chosen
    /// from what the contract declares and the runtime reports", never from code)
    caps: [Cap; KINDS],
    gaze: quality::GazeClassifier,
    /// [Left, Right]
    held: [held::HeldTracker; 2],
    /// direct-touch latch per hand, hysteretic over the near band
    direct: [bool; 2],
    loss: loss::LossTracker,
    state: TierState,
    /// sticky side per family, switched on a rising commit edge (WiVRn, xrdesktop)
    ctrl_side: Side,
    hand_side: Side,
    /// monotone clock: the largest `time_ns` seen, so an out-of-order sample cannot rewind a
    /// timeout
    now_ns: u64,
    /// ticks a pending release has survived (see `loss::expire_pending`)
    pending_ticks: u8,
}

impl Default for Arbiter {
    fn default() -> Self {
        Arbiter::new(TierCfg::default())
    }
}

fn side_index(s: Side) -> usize {
    match s {
        Side::Left => 0,
        Side::Right => 1,
    }
}

fn other(s: Side) -> Side {
    match s {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

impl Arbiter {
    pub fn new(cfg: TierCfg) -> Self {
        Arbiter {
            cfg,
            caps: [Cap::default(); KINDS],
            gaze: quality::GazeClassifier::new(cfg.gaze),
            held: [held::HeldTracker::default(); 2],
            direct: [false; 2],
            loss: loss::LossTracker::default(),
            state: TierState::default(),
            // the default side matters only until the first commit, and only when both sides are
            // available at once (WiVRn's rule picks the lone candidate otherwise). Right is the
            // majority-dominant hand; `Flags::DOMINANT` is the runtime's answer and nothing sets
            // it yet — flagged in the report.
            ctrl_side: Side::Right,
            hand_side: Side::Right,
            now_ns: 0,
            pending_ticks: 0,
        }
    }

    // -- the published answer ------------------------------------------------------------------

    pub fn current(&self) -> Selection {
        self.state.current()
    }

    pub fn state(&self) -> &TierState {
        &self.state
    }

    /// Whether a commit in progress is pinning the tier (§3 line 211).
    pub fn blocks(&self) -> bool {
        self.loss.any_blocks()
    }

    pub fn gaze_state(&self) -> quality::GazeState {
        self.gaze.state()
    }

    /// Whether the gaze *ray* may aim; false through the fallback grace even while gaze holds the
    /// tier, so nothing downstream guesses a pose (§9).
    pub fn gaze_pose_usable(&self) -> bool {
        self.gaze.pose_usable()
    }

    // -- intake --------------------------------------------------------------------------------

    /// One sample. Every kind is folded in — a keyboard key and a controller button while gaze
    /// targets both pass through (`Flow::Continue` in the stage): the commit device need not be
    /// the targeting kind (ADR 0013 amendment item 1). Nothing is marked on the sample; the
    /// stages below read [`Arbiter::current`].
    pub fn observe(&mut self, s: &Sample) -> Outcome {
        self.now_ns = self.now_ns.max(s.time_ns);
        let now = self.now_ns;
        let reports = loss::reports_tracking(s);

        // 1. gestures and loss first: a cancel is emitted *before* any tier change
        let was_committing = self.loss.gesture(s.kind).active();
        // was *anything* already committing when this sample arrived? A commit that begins on
        // this very sample cannot pin the tier to where it was a moment ago — picking a
        // controller up and pressing its trigger is one sample, and the press is what makes it
        // "held" (§3 line 197). A commit already in progress does pin it (§3 lines 210-213).
        let was_pinned = self.loss.any_blocks();
        let release = self.loss.observe(s);
        let now_committing = self.loss.gesture(s.kind).active();
        let rising = (!was_committing && now_committing).then_some(s.kind);

        // 2. the capability table
        self.note(s, reports, now);

        // 3. per-kind trackers
        match s.kind {
            SourceKind::Gaze => {
                if reports {
                    self.gaze.observe(s, now);
                }
            }
            SourceKind::Controller(side) => {
                self.held[side_index(side)].observe(s, reports, &self.cfg.held);
                if !was_committing && now_committing {
                    self.ctrl_side = side;
                }
            }
            SourceKind::Hand(side) => {
                self.update_direct(side, s, reports);
                if !was_committing && now_committing {
                    self.hand_side = side;
                }
            }
            _ => {}
        }

        // 4. arbitrate
        let before = self.state.changes;
        self.resolve(now, if was_pinned { None } else { rising });
        self.state.losses = self.loss.losses;
        Outcome { release, changed: self.state.changes != before, selection: self.state.current() }
    }

    /// The per-tick hook: timeouts fire without a sample (the eyes→head fallback of §3 line 199,
    /// the held timeout, staleness), and a pending release that never came back is expired.
    pub fn tick(&mut self, now_ns: u64) -> Outcome {
        self.now_ns = self.now_ns.max(now_ns);
        let now = self.now_ns;
        if !self.fresh(kind_index(SourceKind::Gaze), now) {
            self.gaze.stale(now);
        }
        self.gaze.tick(now);
        if self.loss.any_pending() {
            self.pending_ticks = self.pending_ticks.saturating_add(1);
            if self.pending_ticks >= 2 {
                self.state.expired += self.loss.expire_pending();
                self.pending_ticks = 0;
            }
        } else {
            self.pending_ticks = 0;
        }
        let before = self.state.changes;
        self.resolve(now, None);
        Outcome { release: None, changed: self.state.changes != before, selection: self.state.current() }
    }

    fn note(&mut self, s: &Sample, reports: bool, now: u64) {
        let c = &mut self.caps[kind_index(s.kind)];
        c.seen = true;
        c.last_ns = now;
        if reports {
            c.tracked = s.tracked;
            c.ready = s.ready;
            c.quality = s.quality;
        }
    }

    /// The near band, hysteretic (`references/wivrn/client/constants.h:45-47`). The distance is
    /// `Values::poke` — metres to the plane along its normal — and it is meaningful only when the
    /// profile gave a poke pose, so the band is evaluated only then (`poke_ext/pose`,
    /// spatial-input §2). Lane B's hit test can supply a truer distance once it exists; the
    /// latch's shape does not change.
    fn update_direct(&mut self, side: Side, s: &Sample, reports: bool) {
        let i = side_index(side);
        if reports && !(s.tracked && s.ready) {
            self.direct[i] = false;
            return;
        }
        if s.poke_pose.is_none() {
            return;
        }
        let d = s.values.poke;
        if self.direct[i] {
            if d > self.cfg.near_leave_m {
                self.direct[i] = false;
            }
        } else if d < self.cfg.near_enter_m {
            self.direct[i] = true;
        }
    }

    // -- the ladder ----------------------------------------------------------------------------

    fn fresh(&self, i: usize, now: u64) -> bool {
        let c = self.caps[i];
        c.seen && now.saturating_sub(c.last_ns) < self.cfg.stale_ns
    }

    fn usable(&self, k: SourceKind, now: u64) -> bool {
        let i = kind_index(k);
        let c = self.caps[i];
        c.tracked && c.ready && self.fresh(i, now)
    }

    /// Tier 1 (§3): gaze, and only when nominal — the extension's own rule, applied in
    /// [`quality`]. A tracked controller does not take it over (ADR item 1; the Meta dissent of
    /// research/63 §1 is not adopted).
    pub fn gaze_targets(&self, now: u64) -> bool {
        self.gaze.holds_tier() && self.fresh(kind_index(SourceKind::Gaze), now)
    }

    /// Tier 2 (§3): a controller that is tracked and in hand ([`held`]).
    pub fn held_controller(&self, now: u64) -> Option<Side> {
        self.pick(self.ctrl_side, |side| {
            self.usable(SourceKind::Controller(side), now) && self.held[side_index(side)].held(now, &self.cfg.held)
        })
    }

    /// Tier 3 (§3): a hand the runtime reports tracked and ready.
    pub fn ready_hand(&self, now: u64) -> Option<Side> {
        self.pick(self.hand_side, |side| self.usable(SourceKind::Hand(side), now))
    }

    /// The direct-touch override (§3 lines 203-208): a ready hand inside the near band.
    pub fn direct_hand(&self, now: u64) -> Option<Side> {
        self.pick(self.hand_side, |side| self.direct[side_index(side)] && self.usable(SourceKind::Hand(side), now))
    }

    /// The sticky-side rule: keep the last one while it qualifies, else take the other if it is
    /// the only candidate (WiVRn `imgui_impl.cpp:726-768`).
    fn pick(&self, sticky: Side, ok: impl Fn(Side) -> bool) -> Option<Side> {
        if ok(sticky) {
            Some(sticky)
        } else if ok(other(sticky)) {
            Some(other(sticky))
        } else {
            None
        }
    }

    /// The precedence ladder itself. Total: the head ray is the floor, so this never fails.
    pub fn would_choose(&self, now: u64) -> (SourceKind, bool) {
        if let Some(side) = self.direct_hand(now) {
            return (SourceKind::Hand(side), true);
        }
        if self.gaze_targets(now) {
            return (SourceKind::Gaze, false);
        }
        if let Some(side) = self.held_controller(now) {
            return (SourceKind::Controller(side), false);
        }
        if let Some(side) = self.ready_hand(now) {
            return (SourceKind::Hand(side), false);
        }
        (SourceKind::Head, false)
    }

    /// `rising` names the kind whose commit began on this very sample, when nothing else was
    /// already committing: such a commit may take the tier with it (and only to itself), but may
    /// not carry it anywhere else.
    fn resolve(&mut self, now: u64, rising: Option<SourceKind>) {
        let (kind, direct) = self.would_choose(now);
        if kind == self.state.targeting && direct == self.state.direct {
            self.state.deferred = None;
            return;
        }
        if self.loss.any_blocks() && rising != Some(kind) {
            // a commit is in progress: the transition waits for the release (§3 lines 210-213)
            if self.state.deferred != Some(kind) {
                self.state.deferrals += 1;
            }
            self.state.deferred = Some(kind);
            return;
        }
        // the selected side becomes the sticky one, so the *other* side appearing does not steal
        // targeting — only a commit on it does (WiVRn `imgui_impl.cpp:726-768`)
        match kind {
            SourceKind::Hand(side) => self.hand_side = side,
            SourceKind::Controller(side) => self.ctrl_side = side,
            _ => {}
        }
        self.state.targeting = kind;
        self.state.class = kind.class();
        self.state.direct = direct;
        self.state.changed_at_ns = now;
        self.state.changes += 1;
        self.state.deferred = None;
    }
}

// ---------------------------------------------------------------------------------------------
// The stage: a thin adapter over the arbiter
// ---------------------------------------------------------------------------------------------

/// `Slot::Tier`. Passes every sample through (`Flow::Continue`) — the arbiter consumes nothing;
/// it decides *which kind targets*, and the stages below it read that decision.
#[derive(Default)]
pub struct TierStage {
    pub arbiter: Arbiter,
}

impl TierStage {
    pub fn new() -> TierStage {
        TierStage { arbiter: Arbiter::default() }
    }

    /// The published answer, for lanes B and C (the hit test and the transports).
    pub fn current(&self) -> Selection {
        self.arbiter.current()
    }

    pub fn state(&self) -> &TierState {
        self.arbiter.state()
    }
}

impl Stage for TierStage {
    fn name(&self) -> &'static str {
        "tier:arbiter"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        let out = self.arbiter.observe(s);
        if let Some(rel) = out.release {
            // the gesture ends before the tier moves: the release is queued here and consumed at
            // the next drain, and `loss` pins the tier until it has been. Lane C maps a
            // release-after-loss (`loss::released_after_loss`) to `wl_touch.cancel` for
            // touch-class kinds and to a plain button release for pointer-class ones.
            tracing::debug!(kind = ?rel.kind, "input tier: source lost mid-gesture, cancelling");
            st.input.push(rel);
        }
        if out.changed {
            // a tier change is an event, not a silent switch (§3 line 210): the cursor appears or
            // disappears (§7) and the shell may show which source is active
            tracing::debug!(targeting = ?out.selection.targeting, class = ?out.selection.class, direct = out.selection.direct, "input tier: change");
        }
        st.input.tier = Some(out.selection);
        Flow::Continue
    }

    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        let out = self.arbiter.tick(now_ns);
        if out.changed {
            tracing::debug!(targeting = ?out.selection.targeting, class = ?out.selection.class, "input tier: change (timeout)");
        }
        st.input.tier = Some(out.selection);
        st.journal.input_tier_changes = self.arbiter.state().changes;
        st.journal.input_tier_deferrals = self.arbiter.state().deferrals;
        st.journal.input_source_losses = self.arbiter.state().losses;
    }
}

// ---------------------------------------------------------------------------------------------
// Tests: the rule is a pure state machine, so all of it is testable without a runtime
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, Values};
    use openxr as xr;

    const MS: u64 = 1_000_000;

    fn state(kind: SourceKind, now: u64, tracked: bool) -> Sample {
        let mut s = Sample::new(kind, now);
        if tracked {
            s.pose = Some(xr::Posef::IDENTITY);
            s.tracked = true;
            s.ready = true;
            s.quality = Quality::Nominal;
        }
        s
    }

    fn gaze(now: u64, q: Quality) -> Sample {
        let mut s = state(SourceKind::Gaze, now, true);
        s.quality = q;
        s.tracked = q == Quality::Nominal;
        s.ready = matches!(q, Quality::Nominal | Quality::SubNominal);
        s
    }

    fn hand(side: Side, now: u64, poke_m: f32) -> Sample {
        let mut s = state(SourceKind::Hand(side), now, true);
        s.poke_pose = Some(xr::Posef::IDENTITY);
        s.values = Values { poke: poke_m, ..Values::default() };
        s
    }

    /// Keep every kind alive across a stretch of time so a staleness timeout does not silently do
    /// the work a tier rule is supposed to do.
    fn keep(a: &mut Arbiter, kinds: &[SourceKind], now: u64) {
        for k in kinds {
            let s = match *k {
                SourceKind::Gaze => gaze(now, Quality::Nominal),
                SourceKind::Hand(side) => hand(side, now, 1.0),
                k => state(k, now, true),
            };
            a.observe(&s);
        }
    }

    #[test]
    fn the_floor_is_the_head_ray() {
        let a = Arbiter::default();
        assert_eq!(a.current().targeting, SourceKind::Head);
        assert_eq!(a.current().class, Class::Pointer);
        assert_eq!(a.would_choose(0), (SourceKind::Head, false));
    }

    /// §3's ladder, walked from the floor up and back down.
    #[test]
    fn precedence_is_gaze_then_controller_then_hand_then_head() {
        let mut a = Arbiter::default();
        let mut now = 0;

        // a tracked hand takes the tier from the head (tier 3)
        a.observe(&hand(Side::Right, now, 1.0));
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right));
        assert_eq!(a.current().class, Class::Touch);

        // a held controller outranks it (tier 2, §3 line 197 + ADR item 1)
        let mut c = state(SourceKind::Controller(Side::Right), now, true);
        c.button = Some((Button::Menu, true));
        a.observe(&c);
        assert_eq!(a.current().targeting, SourceKind::Controller(Side::Right));
        assert_eq!(a.current().class, Class::Pointer, "controllers targeting are pointer-class (§5)");

        // gaze outranks the controller once it has been nominal for the return window (tier 1);
        // Meta's dissent (controllers take targeting back when in hand) is NOT adopted
        for step in 0..=8 {
            now = step * 100 * MS;
            keep(&mut a, &[SourceKind::Controller(Side::Right), SourceKind::Hand(Side::Right)], now);
            a.observe(&gaze(now, Quality::Nominal));
        }
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        assert_eq!(a.current().class, Class::Touch);

        // eyes out: back to the controller after the fallback timeout, and no sooner
        let t0 = now;
        a.observe(&gaze(t0, Quality::SubNominal));
        keep(&mut a, &[SourceKind::Controller(Side::Right)], t0);
        assert_eq!(a.current().targeting, SourceKind::Gaze, "sub-nominal does not drop the tier at once");
        now = t0 + 700 * MS;
        keep(&mut a, &[SourceKind::Controller(Side::Right)], now);
        a.observe(&gaze(now, Quality::SubNominal));
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        now = t0 + 800 * MS;
        a.observe(&gaze(now, Quality::SubNominal));
        keep(&mut a, &[SourceKind::Controller(Side::Right)], now);
        assert_eq!(a.current().targeting, SourceKind::Controller(Side::Right));

        // the controller is put down: nothing happens on it, and after the held timeout the hand
        // ray takes over. The hand is kept alive meanwhile; the controller is not touched.
        let put_down = now;
        while now < put_down + 2_000 * MS {
            now += 300 * MS;
            keep(&mut a, &[SourceKind::Hand(Side::Right)], now);
            a.observe(&gaze(now, Quality::SubNominal));
            a.observe(&state(SourceKind::Controller(Side::Right), now, true));
        }
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right));

        // hands out of view: the floor
        let mut off = Sample::new(SourceKind::Hand(Side::Right), now);
        off.time_ns = now;
        a.observe(&off);
        assert_eq!(a.current().targeting, SourceKind::Head);
    }

    #[test]
    fn the_commit_device_need_not_be_the_targeting_kind() {
        // ADR 0013 amendment item 1: with eyes on, a controller trigger commits *at the gaze
        // target* — the controller does not take targeting, and a keyboard never targets at all.
        let mut a = Arbiter::default();
        let mut now = 0;
        for step in 0..=8 {
            now = step * 100 * MS;
            a.observe(&gaze(now, Quality::Nominal));
        }
        assert_eq!(a.current().targeting, SourceKind::Gaze);

        let mut c = state(SourceKind::Controller(Side::Right), now, true);
        c.button = Some((Button::Select, true));
        a.observe(&c);
        assert_eq!(a.current().targeting, SourceKind::Gaze, "the trigger commits at the gaze target");

        let mut k = Sample::new(SourceKind::Keyboard, now);
        k.key = Some((30, true));
        a.observe(&k);
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        assert!(!SourceKind::Keyboard.targets());
    }

    /// WiVRn's band, both bounds (`constants.h:45-47`).
    #[test]
    fn the_near_band_is_hysteretic_at_0_18_in_and_0_22_out() {
        let mut a = Arbiter::default();
        a.observe(&hand(Side::Right, 0, 0.30));
        assert!(!a.current().direct);
        a.observe(&hand(Side::Right, MS, 0.19));
        assert!(!a.current().direct, "0.19 > 0.18: still a ray");
        a.observe(&hand(Side::Right, 2 * MS, 0.17));
        assert!(a.current().direct, "entered direct touch below 0.18");
        a.observe(&hand(Side::Right, 3 * MS, 0.20));
        assert!(a.current().direct, "0.20 is inside the band: the latch holds");
        a.observe(&hand(Side::Right, 4 * MS, 0.23));
        assert!(!a.current().direct, "left direct touch above 0.22");
    }

    #[test]
    fn direct_touch_overrides_a_gaze_ray() {
        let mut a = Arbiter::default();
        let mut now = 0;
        for step in 0..=8 {
            now = step * 100 * MS;
            a.observe(&gaze(now, Quality::Nominal));
            a.observe(&hand(Side::Right, now, 1.0));
        }
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        now += MS;
        a.observe(&hand(Side::Right, now, 0.10));
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right));
        assert!(a.current().direct);
        assert_eq!(a.current().class, Class::Touch);
        // and the band is left again
        now += MS;
        a.observe(&hand(Side::Right, now, 0.5));
        a.observe(&gaze(now, Quality::Nominal));
        assert_eq!(a.current().targeting, SourceKind::Gaze);
    }

    #[test]
    fn a_transition_never_happens_mid_gesture() {
        let mut a = Arbiter::default();
        let mut now = 0;
        a.observe(&hand(Side::Right, now, 1.0));
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right));

        // pinch closed: a commit is in progress
        let mut p = hand(Side::Right, now, 1.0);
        p.values.pinch = 0.9;
        a.observe(&p);
        assert!(a.blocks());

        // gaze goes nominal and stays so well past the return window — the tier does not move
        for step in 1..=20 {
            now = step * 100 * MS;
            a.observe(&gaze(now, Quality::Nominal));
            let mut p = hand(Side::Right, now, 1.0);
            p.values.pinch = 0.9;
            a.observe(&p);
        }
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right), "no transition mid-pinch");
        assert_eq!(a.state().deferred, Some(SourceKind::Gaze), "the transition is deferred, not dropped");
        assert!(a.state().deferrals >= 1);

        // release: the deferred transition is applied
        now += 100 * MS;
        let mut open = hand(Side::Right, now, 1.0);
        open.values.pinch = 0.0;
        a.observe(&open);
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        assert_eq!(a.state().deferred, None);
    }

    #[test]
    fn loss_mid_gesture_emits_one_release_before_the_tier_changes() {
        let mut a = Arbiter::default();
        let mut now = 0;

        // a held controller targets, with select down
        let mut c = state(SourceKind::Controller(Side::Right), now, true);
        c.button = Some((Button::Select, true));
        let out = a.observe(&c);
        assert!(out.release.is_none());
        assert_eq!(a.current().targeting, SourceKind::Controller(Side::Right));
        let changes_before = a.state().changes;

        // it stops being tracked while select is held
        now += 10 * MS;
        let off = Sample::new(SourceKind::Controller(Side::Right), now);
        let out = a.observe(&off);
        let rel = out.release.expect("a release is synthesised for the interrupted commit");
        assert_eq!(rel.kind, SourceKind::Controller(Side::Right));
        assert_eq!(rel.button, Some((Button::Select, false)));
        assert!(loss::released_after_loss(&rel));
        assert!(!out.changed, "the gesture ends before any tier change");
        assert_eq!(a.state().changes, changes_before);
        assert_eq!(a.state().losses, 1);

        // the release passes through the chain; only then does the tier fall to the floor
        let out = a.observe(&rel);
        assert!(out.release.is_none(), "exactly one release");
        assert!(out.changed);
        assert_eq!(a.current().targeting, SourceKind::Head);
        assert_eq!(a.state().losses, 1);
    }

    #[test]
    fn a_pending_release_that_never_returns_expires_and_unpins_the_tier() {
        let mut a = Arbiter::default();
        let mut h = hand(Side::Right, 0, 1.0);
        h.values.pinch = 0.9;
        a.observe(&h);
        let off = Sample::new(SourceKind::Hand(Side::Right), 10 * MS);
        assert!(a.observe(&off).release.is_some());
        assert!(a.blocks());
        a.tick(20 * MS);
        assert!(a.blocks(), "one tick of grace: the release is drained on the next one");
        a.tick(30 * MS);
        assert!(!a.blocks());
        assert_eq!(a.state().expired, 1);
        assert_eq!(a.current().targeting, SourceKind::Head);
    }

    #[test]
    fn a_put_down_controller_releases_the_tier_after_the_held_timeout() {
        let mut a = Arbiter::default();
        let mut c = state(SourceKind::Controller(Side::Right), 0, true);
        c.button = Some((Button::Select, true));
        a.observe(&c);
        let mut up = state(SourceKind::Controller(Side::Right), MS, true);
        up.button = Some((Button::Select, false));
        a.observe(&up);
        assert_eq!(a.current().targeting, SourceKind::Controller(Side::Right));
        // it lies still: tracked, and nothing happens on it. Samples keep arriving (the runtime
        // locates it every tick), so this isolates the held timeout from the staleness window.
        let mut now = MS;
        for _ in 0..4 {
            now += 400 * MS;
            a.observe(&state(SourceKind::Controller(Side::Right), now, true));
        }
        assert_eq!(now, 1_601 * MS);
        assert_eq!(a.current().targeting, SourceKind::Controller(Side::Right), "1.6 s idle is inside the 2 s stand-in");
        now += 400 * MS;
        a.observe(&state(SourceKind::Controller(Side::Right), now, true));
        assert_eq!(a.current().targeting, SourceKind::Head, "put down: the tier is released");
    }

    #[test]
    fn the_side_is_sticky_and_switches_on_a_commit() {
        // WiVRn imgui_impl.cpp:726-768; xrdesktop xrd-input-synth.c:198-205
        let mut a = Arbiter::default();
        a.observe(&hand(Side::Left, 0, 1.0));
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Left), "the only candidate");
        a.observe(&hand(Side::Right, MS, 1.0));
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Left), "sticky while it qualifies");
        let mut p = hand(Side::Right, 2 * MS, 1.0);
        p.values.pinch = 0.9;
        a.observe(&p);
        assert_eq!(a.current().targeting, SourceKind::Hand(Side::Right), "a rising commit edge switches");
    }

    #[test]
    fn gaze_needs_nominal_and_the_return_window() {
        let mut a = Arbiter::default();
        a.observe(&gaze(0, Quality::SubNominal));
        a.tick(5_000 * MS);
        assert_eq!(a.current().targeting, SourceKind::Head, "sub-nominal never targets");
        let mut now = 5_000 * MS;
        a.observe(&gaze(now, Quality::Nominal));
        assert_eq!(a.current().targeting, SourceKind::Head, "…and nominal is not instant");
        now += 800 * MS;
        a.observe(&gaze(now, Quality::Nominal));
        assert_eq!(a.current().targeting, SourceKind::Gaze);
        assert!(a.gaze_pose_usable());
    }

    // -----------------------------------------------------------------------------------------
    // The property sweep (the shape `scene.rs` uses: a seeded LCG, dependency-free, reproducible)
    // -----------------------------------------------------------------------------------------

    fn family(k: SourceKind) -> usize {
        match k {
            SourceKind::Gaze => 0,
            SourceKind::Controller(_) => 1,
            SourceKind::Hand(_) => 2,
            SourceKind::Head => 3,
            _ => 4,
        }
    }

    /// Every invariant the tier rule must hold, checked after every step of the sweep.
    fn check(a: &Arbiter, now: u64, out: &Outcome, before: Selection, was_pinned: bool) {
        let sel = a.current();

        // 1. exactly one kind targets, and it is one of §3's four tiers
        assert!(family(sel.targeting) < 4, "{:?} is not a targeting tier", sel.targeting);
        assert!(sel.targeting.targets());
        assert_eq!(sel.class, sel.targeting.class(), "the class is the kind's (§5)");
        assert_eq!(sel, out.selection);

        // 2. no transition while a commit is in progress (§3 lines 210-213). "In progress" means
        //    a commit that had already begun when this sample arrived and has not ended: a
        //    commit *beginning* on this sample may carry the tier to its own kind (the trigger
        //    press that is also what makes a controller held), and one *ending* on it releases
        //    the deferred transition.
        if was_pinned && a.blocks() {
            assert!(!out.changed, "the tier moved mid-gesture");
            assert_eq!(sel.targeting, before.targeting);
            assert_eq!(sel.direct, before.direct);
        }

        // 3. a change is an event with a time, and no change leaves the time alone
        if out.changed {
            assert_eq!(sel.changed_at_ns, now);
            assert!(sel.targeting != before.targeting || sel.direct != before.direct);
        } else {
            assert_eq!(sel.changed_at_ns, before.changed_at_ns);
        }

        // 4. precedence, whenever the selection is the ladder's own answer (it differs only while
        //    a transition is deferred)
        let (want, want_direct) = a.would_choose(now);
        if (want, want_direct) == (sel.targeting, sel.direct) {
            match sel.targeting {
                SourceKind::Hand(_) if sel.direct => {}
                SourceKind::Gaze => {
                    assert!(a.gaze_targets(now));
                    assert!(a.direct_hand(now).is_none(), "direct touch outranks a gaze ray");
                }
                SourceKind::Controller(side) => {
                    assert!(!a.gaze_targets(now), "gaze outranks a held controller");
                    assert!(a.direct_hand(now).is_none());
                    assert_eq!(a.held_controller(now), Some(side));
                }
                SourceKind::Hand(side) => {
                    assert!(!a.gaze_targets(now));
                    assert!(a.held_controller(now).is_none(), "a held controller outranks a hand ray");
                    assert_eq!(a.ready_hand(now), Some(side));
                }
                SourceKind::Head => {
                    assert!(!a.gaze_targets(now));
                    assert!(a.held_controller(now).is_none());
                    assert!(a.ready_hand(now).is_none());
                    assert!(a.direct_hand(now).is_none());
                }
                other => panic!("{other:?} cannot target"),
            }
        } else {
            assert!(a.blocks(), "the selection may only lag the ladder while a commit is in progress");
            assert_eq!(a.state().deferred, Some(want));
            assert!(family(want) < 4);
        }

        // 5. gaze targets only while nominal or inside the fallback grace
        if sel.targeting == SourceKind::Gaze {
            assert!(a.gaze_targets(now) || a.blocks());
            assert!(a.gaze_state() == quality::GazeState::Nominal || !a.gaze_pose_usable());
        }
    }

    #[test]
    fn property_sweep_of_the_tier_rule() {
        // a seeded LCG: dependency-free, reproducible (the shape of scene.rs:748-753)
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rnd = move |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n.max(1)
        };

        let mut a = Arbiter::default();
        let mut now: u64 = 0;
        let mut entered = [0u32; 4];
        let mut left = [0u32; 4];
        let mut releases = 0u64;
        let mut ticks = 0u64;

        for _ in 0..20_000 {
            now += rnd(300) * MS;
            let kind = SourceKind::ALL[rnd(SourceKind::ALL.len() as u64) as usize];
            let mut s = Sample::new(kind, now);

            let q = match rnd(6) {
                0 | 1 | 2 => Quality::Nominal,
                3 => Quality::SubNominal,
                4 => Quality::Lost,
                _ => Quality::Unavailable,
            };
            if loss::is_spatial(kind) && rnd(8) != 0 {
                s.quality = q;
                s.tracked = q == Quality::Nominal;
                s.ready = matches!(q, Quality::Nominal | Quality::SubNominal);
                if s.ready {
                    s.pose = Some(xr::Posef::IDENTITY);
                }
            }
            if let SourceKind::Hand(_) = kind {
                if rnd(3) != 0 {
                    s.poke_pose = Some(xr::Posef::IDENTITY);
                    s.values.poke = rnd(60) as f32 / 100.0 - 0.1;
                }
                if rnd(3) == 0 {
                    s.values.pinch = rnd(11) as f32 / 10.0;
                }
            }
            if rnd(5) == 0 {
                s.button = Some((Button::Select, rnd(2) == 1));
            }
            if kind == SourceKind::Keyboard {
                s.key = Some((30, rnd(2) == 1));
            }

            let before = a.current();
            let blocked_before = a.blocks();
            let out = a.observe(&s);
            check(&a, now, &out, before, blocked_before);

            if let Some(rel) = out.release.clone() {
                releases += 1;
                assert!(!out.changed, "a loss cancels before the tier moves");
                assert!(blocked_before, "a release only follows a commit in progress");
                assert!(a.blocks(), "the tier stays pinned until the release lands");
                // the release goes back through the chain, as `Input::queue` does
                let before = a.current();
                let pinned = a.blocks();
                let out = a.observe(&rel);
                check(&a, a.now_ns, &out, before, pinned);
            }

            if out.changed {
                left[family(before.targeting)] += 1;
                entered[family(a.current().targeting)] += 1;
            }

            if rnd(4) == 0 {
                ticks += 1;
                let before = a.current();
                let pinned = a.blocks();
                let out = a.tick(now);
                check(&a, now, &out, before, pinned);
                if out.changed {
                    left[family(before.targeting)] += 1;
                    entered[family(a.current().targeting)] += 1;
                }
            }
        }

        // every tier is reachable and reversible
        for (i, name) in ["gaze", "controller", "hand", "head"].iter().enumerate() {
            assert!(entered[i] > 0, "tier {name} was never entered");
            assert!(left[i] > 0, "tier {name} was never left");
        }
        assert!(releases > 0, "the sweep never exercised a loss mid-gesture");
        assert!(a.state().deferrals > 0, "the sweep never exercised a deferred transition");
        assert!(ticks > 0);
        assert_eq!(a.state().losses, releases, "one release per loss, no more");
    }
}
