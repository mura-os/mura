//! The frame journal (specs/zxr-core.md §11): per-frame timing and the counters the R0 gates
//! read. Printed as `key=value` lines on SIGUSR1 and at exit; the harness parses them.

use std::fmt::Write as _;

/// A latency histogram for one runtime call (research/63 Phase 0): count, sum, max and
/// log2 buckets from < 16 µs to ≥ 2 ms, so the per-call IPC cost is visible per tick.
#[derive(Default, Debug, Clone)]
pub struct Lat {
    pub n: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
    /// bucket i counts samples in [16µs·2^(i−1), 16µs·2^i); bucket 0 is < 16 µs, bucket 7 is ≥ 1 ms
    pub buckets: [u64; 8],
}

impl Lat {
    pub fn add(&mut self, ns: u64) {
        self.n += 1;
        self.sum_ns += ns;
        self.max_ns = self.max_ns.max(ns);
        let us = ns / 1000;
        let b = if us < 16 { 0 } else { ((us / 16).ilog2() as usize + 1).min(7) };
        self.buckets[b] += 1;
    }
    fn render(&self, s: &mut String, name: &str) {
        let mean = if self.n > 0 { self.sum_ns / self.n / 1000 } else { 0 };
        let _ = writeln!(s, "call_{name}_n={} call_{name}_us_mean={} call_{name}_us_max={} call_{name}_hist={}", self.n, mean, self.max_ns / 1000, self.buckets.iter().map(|b| b.to_string()).collect::<Vec<_>>().join("/"));
    }
}

/// The OpenXR calls a tick makes, each a round trip to the runtime over Monado's IPC.
#[derive(Default, Debug, Clone)]
pub struct Calls {
    pub wait_frame: Lat,
    pub begin_frame: Lat,
    pub locate_views: Lat,
    pub locate_spaces: Lat,
    pub acquire_image: Lat,
    pub wait_image: Lat,
    pub release_image: Lat,
    pub end_frame: Lat,
    pub poll_event: Lat,
    /// `xrSyncActions`, once per tick (spatial-input §1a): N_devices `update_inputs` RPCs on
    /// Monado (`oxr_input.c:2045-2050`, `ipc_client_xdev.c:37-70`) — the upstream batching item
    pub sync_actions: Lat,
    /// `xrGetActionState*` / `xrGetCurrentInteractionProfile`: client-side on Monado (no round trip), recorded, not in the census
    pub get_action_state: Lat,
    /// `xrLocateHandJointsEXT`, one per tracked hand per tick while the §10 bridge runs
    pub hand_joints: Lat,
}

/// Time a closure and record it in a `Lat`.
pub fn timed<T>(lat: &mut Lat, f: impl FnOnce() -> T) -> T {
    let t0 = crate::state::now_ns();
    let r = f();
    lat.add(crate::state::now_ns().saturating_sub(t0));
    r
}

#[derive(Default, Debug, Clone)]
pub struct Journal {
    pub calls: Calls,
    /// runtime round trips this process has made (all `Calls` counts summed at render time)
    /// analytic attachment traffic per rendered frame (bytes): Σ over passes of
    /// colour/depth loads + stores at the pass extent — what a tile-based GPU would move
    pub attachment_bytes_est: u64,
    pub passes_per_frame: u64,
    pub frame_callbacks_visible: u64,
    pub frame_callbacks_occluded: u64,
    pub panel_passes: u64,
    pub panel_bytes: u64,
    pub panel_swapchains: u64,
    /// spec §7 rev 3: which shape each tick took
    pub projection_layer_frames: u64,
    pub panels_only_frames: u64,
    /// ticks in quiet mode (a native app primary): zero layers, no passes
    pub quiet_frames: u64,
    /// research/69: buffers held at commit for surfaces zxr was not sampling (policy != replacement)
    pub held_unsampled: u64,
    /// research/69: most client buffers held at once (all lists)
    pub held_outstanding_max: u64,
    /// research/69 §3: xdg_toplevel.suspended state changes sent (quiet / hidden)
    pub suspended_configures: u64,
    /// spec §5a scene counters (§11 rev 3.3)
    pub members_composed: u64,
    pub members_dirty: u64,
    pub quads_submitted: u64,
    pub overflow: u64,
    pub panel_acquires: u64,
    pub panel_releases: u64,
    pub panel_swapchains_created: u64,
    pub panel_swapchains_destroyed: u64,
    pub panel_swapchains_grown: u64,
    pub panel_swapchains_shrunk: u64,
    /// commits whose root could not be resolved: every mapped member marked dirty
    pub dirty_fallbacks: u64,
    /// ticks that made the batched `xrLocateSpacesKHR` call (only with `Xr` frames present)
    pub locate_spaces_ticks: u64,
    /// input module (spec §11 rev 3.5): samples through the chain, and how many each slot consumed
    pub input_samples: u64,
    pub input_consumed: [u64; 9],
    pub input_presence_changes: u64,
    /// intake latency of event samples (libinput/EI/injector): event timestamp → the tick that ran it
    pub input_events: u64,
    pub input_event_age_ns_total: u64,
    pub input_event_age_ns_max: u64,
    /// event timestamp → the consuming tick's completed `xrEndFrame` (the gate's trigger number)
    pub input_event_to_end_n: u64,
    pub input_event_to_end_ns_total: u64,
    pub input_event_to_end_ns_max: u64,
    /// the same interval per event (every event of the tick, not only the oldest)
    pub input_event_to_end_per_event_n: u64,
    pub input_event_to_end_per_event_ns_total: u64,
    /// input tier (spatial-input §3): targeting-source changes, transitions deferred by a commit
    /// in progress, and sources that lost tracking mid-gesture
    pub input_tier_changes: u64,
    pub input_tier_deferrals: u64,
    pub input_source_losses: u64,
    /// the transports (spatial-input §5): touch contacts opened and cancelled, gaze-scroll exceptions,
    /// logical-pointer handoffs and warps
    pub input_touch_downs: u64,
    pub input_touch_cancels: u64,
    pub input_gaze_scrolls: u64,
    pub input_pointer_handoffs: u64,
    pub input_pointer_warps: u64,
    /// ray-owned pointers released (`leave`) when gaze took the tier (spatial-input §5)
    pub input_pointer_releases: u64,
    /// ticks a `cursor-shape-v1` name was the client cursor and went unrendered (§7 theme open)
    pub input_cursor_named_ticks: u64,
    /// the cursor (spatial-input §7; research/70 §9): cursor quad layers submitted (≤ 1 per
    /// frame — the one-element rule), passes into the cursor panel (content changes only, never
    /// motion), and cursor swapchains created (1 per session unless a client image outgrows the
    /// fixed panel)
    pub cursor_layers: u64,
    pub cursor_passes: u64,
    pub cursor_swapchains_created: u64,
    /// settings (research/73 §6): keys resolved from the artifact, reloads on a store change,
    /// stored values the engine reported invalid, and the generation counter the stages compare
    pub settings_keys: u64,
    pub settings_reloads: u64,
    pub settings_invalid: u64,
    pub settings_generation: u64,
    /// the window grab (wm §4a, input/grabs.rs): grabs begun (of which from client requests),
    /// released, resize steps, depth pushes, and pose updates while grabbed
    pub grab_requests: u64,
    pub grabs_started: u64,
    pub grabs_from_requests: u64,
    pub grabs_released: u64,
    pub grab_resizes: u64,
    pub grab_pushes: u64,
    pub grab_moves: u64,
    /// bar quads submitted (≤ 1 per frame)
    pub grab_bar_layers: u64,
    /// the shell layer (spec §11 rev 3.12, research/77 §7): layer surfaces created / mapped /
    /// unmapped, arrangement runs (layer events only — a still session adds none), configures
    /// sent, exclusive-override changes, registry/bind decisions that hid a privileged global,
    /// clients inserted restricted / trusted, trusted clients lost, security contexts created,
    /// and `wl_pointer.motion`s the still-pointer rule dropped (research/75 D3 made visible)
    pub layer_surfaces: u64,
    pub layer_mapped: u64,
    pub layer_unmapped: u64,
    pub layer_arranges: u64,
    pub layer_configures: u64,
    pub layer_focus_overrides: u64,
    pub binds_filtered: u64,
    pub clients_restricted: u64,
    pub clients_trusted: u64,
    pub trusted_lost: u64,
    /// OSK child restarts (KWin's bound; filter.rs `restart_osk`)
    pub osk_restarts: u64,
    /// the OSK member raised above the surface it types into (phoc's rule; shell/mod.rs)
    pub osk_raises: u64,
    /// lock triggers fired: `session.lock.on_doff` after the grace, `on_idle` after the idle ladder, `zxr ctl lock`
    pub lock_triggers: u64,
    /// `ext_session_lock_v1.lock` accepted after the previous locker died (Defunct)
    pub lock_relocks: u64,
    /// re-seats of the shell's world anchor by recenter (shell/anchor.rs `reseat`)
    pub anchor_reseats: u64,
    /// re-poses of a `typed` member under a moving window (shell/mod.rs `typed_tick`)
    pub osk_follows: u64,
    pub security_contexts: u64,
    pub pointer_motion_deduped: u64,
    /// whether the previous tick submitted GPU work (the timestamps are valid only then)
    pub last_tick_submitted: bool,
    pub frames: u64,
    pub frames_rendered: u64,
    pub missed_deadlines: u64,
    pub gpu_ns_total: u64,
    pub gpu_ns_max: u64,
    pub wake_to_end_ns_total: u64,
    pub wake_to_end_ns_max: u64,
    pub shm_uploads: u64,
    pub dmabuf_imports: u64,
    pub dmabuf_cpu_copies: u64,
    pub acquire_syncobj: u64,
    pub acquire_implicit: u64,
    pub buffers_released: u64,
    pub retention_frames_total: u64,
    pub retention_frames_max: u64,
    pub frame_callbacks: u64,
    pub commits: u64,
    pub toplevels_mapped: u64,
    pub toplevels_unmapped: u64,
    pub popups: u64,
    pub focus_changes: u64,
    pub xwayland_toplevels: u64,
    pub fences_signalled: u64,
    pub fences_outstanding: u64,
    pub stale_texture_draws: u64,
    pub started_at_ns: u64,
}

impl Journal {
    pub fn record_frame(&mut self, rendered: bool, gpu_ns: Option<u64>, wake_to_end_ns: u64, missed: bool) {
        self.frames += 1;
        if rendered {
            self.frames_rendered += 1;
        }
        if let Some(g) = gpu_ns {
            self.gpu_ns_total += g;
            self.gpu_ns_max = self.gpu_ns_max.max(g);
        }
        self.wake_to_end_ns_total += wake_to_end_ns;
        self.wake_to_end_ns_max = self.wake_to_end_ns_max.max(wake_to_end_ns);
        if missed {
            self.missed_deadlines += 1;
        }
    }

    pub fn record_release(&mut self, retained_frames: u64) {
        self.buffers_released += 1;
        self.retention_frames_total += retained_frames;
        self.retention_frames_max = self.retention_frames_max.max(retained_frames);
    }

    pub fn render(&self, now_ns: u64) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "frames={}", self.frames);
        let _ = writeln!(s, "frames_rendered={}", self.frames_rendered);
        let _ = writeln!(s, "missed_deadlines={}", self.missed_deadlines);
        let _ = writeln!(s, "gpu_us_mean={}", if self.frames_rendered > 0 { self.gpu_ns_total / self.frames_rendered / 1000 } else { 0 });
        let _ = writeln!(s, "gpu_us_max={}", self.gpu_ns_max / 1000);
        let _ = writeln!(s, "wake_to_end_us_mean={}", if self.frames > 0 { self.wake_to_end_ns_total / self.frames / 1000 } else { 0 });
        let _ = writeln!(s, "wake_to_end_us_max={}", self.wake_to_end_ns_max / 1000);
        let _ = writeln!(s, "shm_uploads={}", self.shm_uploads);
        let _ = writeln!(s, "dmabuf_imports={}", self.dmabuf_imports);
        let _ = writeln!(s, "dmabuf_cpu_copies={}", self.dmabuf_cpu_copies);
        let _ = writeln!(s, "acquire_syncobj={}", self.acquire_syncobj);
        let _ = writeln!(s, "acquire_implicit={}", self.acquire_implicit);
        let _ = writeln!(s, "buffers_released={}", self.buffers_released);
        let _ = writeln!(s, "retention_frames_mean_x100={}", if self.buffers_released > 0 { self.retention_frames_total * 100 / self.buffers_released } else { 0 });
        let _ = writeln!(s, "retention_frames_max={}", self.retention_frames_max);
        let _ = writeln!(s, "frame_callbacks={}", self.frame_callbacks);
        let _ = writeln!(s, "commits={}", self.commits);
        let _ = writeln!(s, "toplevels_mapped={}", self.toplevels_mapped);
        let _ = writeln!(s, "toplevels_unmapped={}", self.toplevels_unmapped);
        let _ = writeln!(s, "popups={}", self.popups);
        let _ = writeln!(s, "focus_changes={}", self.focus_changes);
        let _ = writeln!(s, "xwayland_toplevels={}", self.xwayland_toplevels);
        let _ = writeln!(s, "fences_signalled={}", self.fences_signalled);
        let _ = writeln!(s, "fences_outstanding={}", self.fences_outstanding);
        let _ = writeln!(s, "stale_texture_draws={}", self.stale_texture_draws);
        let _ = writeln!(s, "uptime_ms={}", now_ns.saturating_sub(self.started_at_ns) / 1_000_000);
        // runtime calls (research/63 §1): per-call latency and the per-frame census
        let c = &self.calls;
        let total = c.wait_frame.n + c.begin_frame.n + c.locate_views.n + c.locate_spaces.n + c.acquire_image.n + c.wait_image.n + c.release_image.n + c.end_frame.n + c.poll_event.n + c.sync_actions.n + c.hand_joints.n;
        let _ = writeln!(s, "runtime_calls_total={}", total);
        let _ = writeln!(s, "runtime_calls_per_frame_x100={}", if self.frames > 0 { total * 100 / self.frames } else { 0 });
        let blocking_ns = c.begin_frame.sum_ns + c.locate_views.sum_ns + c.locate_spaces.sum_ns + c.acquire_image.sum_ns + c.wait_image.sum_ns + c.release_image.sum_ns + c.end_frame.sum_ns + c.sync_actions.sum_ns + c.hand_joints.sum_ns;
        let _ = writeln!(s, "runtime_calls_loop_us_per_frame={}", if self.frames > 0 { blocking_ns / self.frames / 1000 } else { 0 });
        c.wait_frame.render(&mut s, "wait_frame");
        c.begin_frame.render(&mut s, "begin_frame");
        c.locate_views.render(&mut s, "locate_views");
        c.locate_spaces.render(&mut s, "locate_spaces");
        c.acquire_image.render(&mut s, "acquire_image");
        c.wait_image.render(&mut s, "wait_image");
        c.release_image.render(&mut s, "release_image");
        c.end_frame.render(&mut s, "end_frame");
        c.poll_event.render(&mut s, "poll_event");
        c.sync_actions.render(&mut s, "sync_actions");
        c.get_action_state.render(&mut s, "get_action_state");
        c.hand_joints.render(&mut s, "hand_joints");
        // GPU structure (research/63 §2): analytic, not measured
        let _ = writeln!(s, "passes_per_frame={}", self.passes_per_frame);
        let _ = writeln!(s, "attachment_bytes_est_per_frame={}", self.attachment_bytes_est);
        let _ = writeln!(s, "frame_callbacks_visible={}", self.frame_callbacks_visible);
        let _ = writeln!(s, "frame_callbacks_occluded={}", self.frame_callbacks_occluded);
        let _ = writeln!(s, "panel_swapchains={}", self.panel_swapchains);
        let _ = writeln!(s, "panel_passes={}", self.panel_passes);
        let _ = writeln!(s, "panel_bytes={}", self.panel_bytes);
        let _ = writeln!(s, "projection_layer_frames={}", self.projection_layer_frames);
        let _ = writeln!(s, "panels_only_frames={}", self.panels_only_frames);
        let _ = writeln!(s, "quiet_frames={}", self.quiet_frames);
        let _ = writeln!(s, "held_unsampled={}", self.held_unsampled);
        let _ = writeln!(s, "held_outstanding_max={}", self.held_outstanding_max);
        let _ = writeln!(s, "suspended_configures={}", self.suspended_configures);
        // scene (spec §5a / §11 rev 3.3): per-tick means ×100 where a mean is the useful form
        let f = self.frames.max(1);
        let secs = (now_ns.saturating_sub(self.started_at_ns) / 1_000_000_000).max(1);
        let _ = writeln!(s, "members_composed_per_frame_x100={}", self.members_composed * 100 / f);
        let _ = writeln!(s, "members_dirty_per_frame_x100={}", self.members_dirty * 100 / f);
        let _ = writeln!(s, "members_dirty_total={}", self.members_dirty);
        let _ = writeln!(s, "quads_submitted_per_frame_x100={}", self.quads_submitted * 100 / f);
        let _ = writeln!(s, "overflow_per_frame_x100={}", self.overflow * 100 / f);
        let _ = writeln!(s, "panel_acquires={}", self.panel_acquires);
        let _ = writeln!(s, "panel_releases={}", self.panel_releases);
        let _ = writeln!(s, "panel_swapchains_created={}", self.panel_swapchains_created);
        let _ = writeln!(s, "panel_swapchains_destroyed={}", self.panel_swapchains_destroyed);
        let _ = writeln!(s, "panel_swapchains_grown={}", self.panel_swapchains_grown);
        let _ = writeln!(s, "panel_swapchains_shrunk={}", self.panel_swapchains_shrunk);
        let _ = writeln!(s, "panel_swapchains_churn_per_s_x100={}", (self.panel_swapchains_created + self.panel_swapchains_destroyed) * 100 / secs);
        let _ = writeln!(s, "dirty_fallbacks={}", self.dirty_fallbacks);
        let _ = writeln!(s, "locate_spaces_ticks={}", self.locate_spaces_ticks);
        let _ = writeln!(s, "input_samples={}", self.input_samples);
        let _ = writeln!(s, "input_samples_per_frame_x100={}", self.input_samples * 100 / f);
        let _ = writeln!(s, "input_consumed_by_slot={}", self.input_consumed.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
        let _ = writeln!(s, "input_presence_changes={}", self.input_presence_changes);
        let _ = writeln!(s, "input_events={}", self.input_events);
        let _ = writeln!(s, "input_event_age_us_mean={}", if self.input_events > 0 { self.input_event_age_ns_total / self.input_events / 1000 } else { 0 });
        let _ = writeln!(s, "input_event_age_us_max={}", self.input_event_age_ns_max / 1000);
        let _ = writeln!(s, "input_event_to_end_us_mean={}", if self.input_event_to_end_n > 0 { self.input_event_to_end_ns_total / self.input_event_to_end_n / 1000 } else { 0 });
        let _ = writeln!(s, "input_event_to_end_us_max={}", self.input_event_to_end_ns_max / 1000);
        let _ = writeln!(s, "input_event_to_end_per_event_us_mean={}", if self.input_event_to_end_per_event_n > 0 { self.input_event_to_end_per_event_ns_total / self.input_event_to_end_per_event_n / 1000 } else { 0 });
        let _ = writeln!(s, "input_tier_changes={}", self.input_tier_changes);
        let _ = writeln!(s, "input_tier_deferrals={}", self.input_tier_deferrals);
        let _ = writeln!(s, "input_source_losses={}", self.input_source_losses);
        let _ = writeln!(s, "input_touch_downs={}", self.input_touch_downs);
        let _ = writeln!(s, "input_touch_cancels={}", self.input_touch_cancels);
        let _ = writeln!(s, "input_gaze_scrolls={}", self.input_gaze_scrolls);
        let _ = writeln!(s, "input_pointer_handoffs={}", self.input_pointer_handoffs);
        let _ = writeln!(s, "input_pointer_warps={}", self.input_pointer_warps);
        let _ = writeln!(s, "input_pointer_releases={}", self.input_pointer_releases);
        let _ = writeln!(s, "input_cursor_named_ticks={}", self.input_cursor_named_ticks);
        let _ = writeln!(s, "cursor_layers={}", self.cursor_layers);
        let _ = writeln!(s, "cursor_layers_per_frame_x100={}", self.cursor_layers * 100 / f);
        let _ = writeln!(s, "cursor_passes={}", self.cursor_passes);
        let _ = writeln!(s, "cursor_swapchains_created={}", self.cursor_swapchains_created);
        let _ = writeln!(s, "settings_keys={}", self.settings_keys);
        let _ = writeln!(s, "settings_reloads={}", self.settings_reloads);
        let _ = writeln!(s, "settings_invalid={}", self.settings_invalid);
        let _ = writeln!(s, "settings_generation={}", self.settings_generation);
        let _ = writeln!(s, "grab_requests={}", self.grab_requests);
        let _ = writeln!(s, "grabs_started={}", self.grabs_started);
        let _ = writeln!(s, "grabs_from_requests={}", self.grabs_from_requests);
        let _ = writeln!(s, "grabs_released={}", self.grabs_released);
        let _ = writeln!(s, "grab_resizes={}", self.grab_resizes);
        let _ = writeln!(s, "grab_pushes={}", self.grab_pushes);
        let _ = writeln!(s, "grab_moves={}", self.grab_moves);
        let _ = writeln!(s, "grab_bar_layers={}", self.grab_bar_layers);
        for (k, v) in [
            ("layer_surfaces", self.layer_surfaces),
            ("layer_mapped", self.layer_mapped),
            ("layer_unmapped", self.layer_unmapped),
            ("layer_arranges", self.layer_arranges),
            ("layer_configures", self.layer_configures),
            ("layer_focus_overrides", self.layer_focus_overrides),
            ("binds_filtered", self.binds_filtered),
            ("clients_restricted", self.clients_restricted),
            ("clients_trusted", self.clients_trusted),
            ("trusted_lost", self.trusted_lost),
            ("osk_restarts", self.osk_restarts),
            ("osk_raises", self.osk_raises),
            ("lock_triggers", self.lock_triggers),
            ("lock_relocks", self.lock_relocks),
            ("anchor_reseats", self.anchor_reseats),
            ("osk_follows", self.osk_follows),
            ("security_contexts", self.security_contexts),
            ("pointer_motion_deduped", self.pointer_motion_deduped),
        ] {
            let _ = writeln!(s, "{k}={v}");
        }
        s
    }
}
