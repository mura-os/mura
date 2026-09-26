//! User activity — the input side of the idle ladder (ADR 0007 lines 98-102: "idle-past-lock…
//! Serve `ext-idle-notify-v1`, honor `zwp_idle_inhibit_v1`").
//!
//! **Shape, ported.** KWin's `UserActivitySpy` is installed as a **spy**, not a filter
//! (`references/kwin/src/input.cpp:3169-3172`, next to `HideCursorSpy` and *before* the lock-screen
//! filter at 3175-3178): a spy sees every event, including the ones a later filter eats, and
//! consumes nothing (`references/kwin/src/input.h:397-411` describes the filter contract a spy is
//! deliberately not part of). So in zxr's chain this is **a side effect, not a slot**: no
//! `Slot::Activity` exists and none should. Every stage that recognises an event calls in here;
//! `Reserved` does so for the presses it consumes, and lane C's seat stage must do so for every
//! sample it delivers — the exact call is
//!
//! ```ignore
//! crate::input::activity::notify_flagged(st, s.time_ns, s.flags);
//! ```
//!
//! placed where the seat has decided to emit, not where it has decided to drop.
//!
//! **Emulated input.** `Flags::EMULATED` marks a sample from an EI client. libei exists to give
//! the compositor exactly this distinction: "separation of emulated input from normal input.
//! Emulated input is a distinct channel for the compositor and can thus be handled accordingly…
//! Each libei client has its own input device set, the server is always aware of which client is
//! requesting input… The server is in control of emulated input - it can filter input or discard
//! at will" (`references/libei/README.md:32-71`). So emulated activity is **tallied separately**
//! and counts towards the idle timer only while [`Activity::count_emulated`] is set. Its default
//! (`true` — an EI client's synthetic motion keeps the session awake, as on mutter and KWin, where
//! EI events enter the same spy path) is a **flagged judgment**: no comparable states a rule for
//! an XR headset whose idle ladder also locks.
//!
//! **Where the state lives — flagged.** `Zxr` has no activity field and `state.rs`/`input/mod.rs`
//! are not this lane's to edit, so `notify` parks its record in a per-thread cell that the
//! `Mode` stage folds into its own [`Activity`] at each tick ([`Activity::absorb_pending`]). The
//! state loop is single-threaded, so the cell is exactly one `u64` pair and costs nothing; it is
//! still a hidden place for state that belongs on `Input`. The additive patch that removes it is
//! in this lane's report.

use std::cell::Cell;

use super::Flags;
use crate::state::Zxr;

/// The last-activity record. Held by the `Mode` stage (`mode.rs`), read by the lock machine's
/// idle ladder and by `ext-idle-notify`'s timers once lane F lands them.
pub struct Activity {
    last_ns: u64,
    events: u64,
    emulated_events: u64,
    /// whether a sample marked `Flags::EMULATED` moves `last_ns` (see the module note)
    pub count_emulated: bool,
}

impl Default for Activity {
    fn default() -> Self {
        Activity { last_ns: 0, events: 0, emulated_events: 0, count_emulated: true }
    }
}

impl Activity {
    /// CLOCK_MONOTONIC ns of the last input that counted. 0 = nothing yet.
    pub fn last_activity(&self) -> u64 {
        self.last_ns
    }

    pub fn events(&self) -> u64 {
        self.events
    }

    /// Events that arrived marked `Flags::EMULATED` (libei's "distinction"), counted whether or
    /// not they moved `last_ns`.
    pub fn emulated_events(&self) -> u64 {
        self.emulated_events
    }

    /// ns since the last counted input, 0 if none.
    pub fn idle_ns(&self, now_ns: u64) -> u64 {
        if self.last_ns == 0 { 0 } else { now_ns.saturating_sub(self.last_ns) }
    }

    pub fn record(&mut self, now_ns: u64, flags: Flags) {
        self.events += 1;
        if flags.contains(Flags::EMULATED) {
            self.emulated_events += 1;
            if !self.count_emulated {
                return;
            }
        }
        self.last_ns = self.last_ns.max(now_ns);
    }

    /// Fold in whatever [`notify`] recorded since the last tick.
    pub fn absorb_pending(&mut self) {
        PENDING.with(|p| {
            let (last, n, emulated) = p.take();
            if n == 0 {
                return;
            }
            self.events += n;
            self.emulated_events += emulated;
            self.last_ns = self.last_ns.max(last);
        });
    }
}

thread_local! {
    /// (last counted ns, events, emulated events) since the last `absorb_pending`
    static PENDING: Cell<(u64, u64, u64)> = const { Cell::new((0, 0, 0)) };
}

/// Record user activity. The call every stage makes; see the module note on where it lands.
pub fn notify(st: &mut Zxr, now_ns: u64) {
    notify_flagged(st, now_ns, Flags::default());
}

/// [`notify`] keeping the sample's provenance (`Flags::EMULATED`, `libei/README.md:53-71`).
pub fn notify_flagged(st: &mut Zxr, now_ns: u64, flags: Flags) {
    // `st` is taken because this is the call site lane C and lane F will use and because honouring
    // `zwp_idle_inhibit_v1` reads compositor state (the inhibiting surface's mapped-ness); neither
    // protocol state exists on `Zxr` yet — the patch is in this lane's report.
    let _ = st;
    record_pending(now_ns, flags);
}

/// The cell write [`notify`] makes, without the `&mut Zxr` no stage has in a unit test.
pub fn record_pending(now_ns: u64, flags: Flags) {
    PENDING.with(|p| {
        let (last, n, emulated) = p.get();
        let emulated_now = u64::from(flags.contains(Flags::EMULATED));
        p.set((last.max(now_ns), n + 1, emulated + emulated_now));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_tracks_the_latest_and_tallies_emulated() {
        let mut a = Activity::default();
        assert_eq!(a.last_activity(), 0);
        assert_eq!(a.idle_ns(1_000), 0, "no input yet is not idle time");
        a.record(100, Flags::default());
        a.record(50, Flags::default());
        assert_eq!(a.last_activity(), 100, "out-of-order samples never move it backwards");
        assert_eq!(a.events(), 2);
        let mut e = Flags::default();
        e.insert(Flags::EMULATED);
        a.record(200, e);
        assert_eq!(a.last_activity(), 200);
        assert_eq!(a.emulated_events(), 1);
        assert_eq!(a.idle_ns(500), 300);
        a.count_emulated = false;
        a.record(900, e);
        assert_eq!(a.last_activity(), 200, "emulated input discarded when the policy says so");
        assert_eq!(a.emulated_events(), 2, "but still distinguished (libei README:53-71)");
    }
}
