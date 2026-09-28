//! The scene's state machine: one `App` on the UI thread, reached from the backend threads
//! through [`with`] (they post closures with `slint::invoke_from_event_loop`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::accounts::{self, Account};
use crate::conv::{self, Backend, Event};
use crate::logind::{Logind, Power, Signal};
use crate::sessions::SessionEntry;
use crate::{AccountEntry, Greeter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Greeter,
    Lock { lock_now: bool },
}

pub struct App {
    ui: Greeter,
    mode: Mode,
    handle: mura_slint_platform::Handle,
    accounts: Vec<Account>,
    sessions: Vec<SessionEntry>,
    backend: Option<Backend>,
    username: String,
    logind: Option<Logind>,
    retry_timer: slint::Timer,
    power_timer: slint::Timer,
    clock_timer: slint::Timer,
    pending_power: Option<Power>,
    zxr_control: Option<String>,
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

/// Run `f` on the app, from the UI thread.
pub fn with(f: impl FnOnce(&mut App)) {
    let app = APP.with(|a| a.borrow().clone());
    if let Some(app) = app {
        f(&mut app.borrow_mut());
    }
}

/// The failure text every user sees (tuigreet's leak argument; multi-user §2).
const GENERIC_FAILURE: &str = "Authentication failed";
/// cosmic's power dialog countdown.
const POWER_COUNTDOWN_S: i32 = 10;

impl App {
    pub fn install(ui: Greeter, mode: Mode, handle: mura_slint_platform::Handle) -> Rc<RefCell<App>> {
        let logind = match Logind::connect() {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!("logind unavailable: {e}");
                None
            }
        };
        let app = Rc::new(RefCell::new(App {
            ui,
            mode,
            handle,
            accounts: Vec::new(),
            sessions: Vec::new(),
            backend: None,
            username: String::new(),
            logind,
            retry_timer: slint::Timer::default(),
            power_timer: slint::Timer::default(),
            clock_timer: slint::Timer::default(),
            pending_power: None,
            zxr_control: std::env::var("ZXR_CONTROL").ok(),
        }));
        APP.with(|a| *a.borrow_mut() = Some(app.clone()));
        app.borrow_mut().start();
        app
    }

    fn start(&mut self) {
        let ui = self.ui.clone_strong();
        ui.set_lock_mode(matches!(self.mode, Mode::Lock { .. }));
        ui.set_hostname(accounts::hostname().into());
        ui.set_dwell_available(self.zxr_control.is_some());
        if let Some(l) = &self.logind {
            ui.set_can_poweroff(l.can(Power::PowerOff));
            ui.set_can_reboot(l.can(Power::Reboot));
            ui.set_can_suspend(l.can(Power::Suspend));
        }
        self.tick_clock();
        self.clock_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || with(|a| a.tick_clock()));

        // callbacks
        ui.on_select_account(|name| with(|a| a.select_account(name.to_string())));
        ui.on_submit_username(|name| with(|a| a.submit_username(name.to_string())));
        ui.on_respond(|text| with(|a| a.respond(text.to_string())));
        ui.on_cancel(|| with(|a| a.cancel()));
        ui.on_backspace(|| {
            with(|a| {
                let mut s = a.ui.get_response().to_string();
                s.pop();
                a.ui.set_response(s.into());
            })
        });
        ui.on_choose_session(|i| tracing::info!(index = i, "session chosen"));
        ui.on_power(|kind| with(|a| a.power_requested(kind.as_str())));
        ui.on_confirm_power(|| with(|a| a.power_confirmed()));
        ui.on_cancel_power(|| with(|a| a.power_cancelled()));
        ui.on_toggle_dwell(|on| with(|a| a.toggle_dwell(on)));

        match self.mode {
            Mode::Greeter => {
                self.accounts = accounts::enumerate();
                self.sessions = crate::sessions::load();
                let entries: Vec<AccountEntry> = self.accounts.iter().map(|a| AccountEntry { name: a.name.clone().into(), display: a.display.clone().into() }).collect();
                ui.set_accounts(ModelRc::new(VecModel::from(entries)));
                ui.set_picker_visible(self.accounts.len() >= 2);
                let names: Vec<SharedString> = self.sessions.iter().map(|s| s.name.clone().into()).collect();
                ui.set_sessions(ModelRc::new(VecModel::from(names)));
                ui.set_chooser_visible(self.sessions.len() >= 2);
                if let Some(last) = accounts::last_user() {
                    if self.accounts.iter().any(|a| a.name == last) || self.accounts.is_empty() {
                        ui.set_username(last.into());
                    }
                }
                tracing::info!(accounts = self.accounts.len(), sessions = self.sessions.len(), "greeter mode");
            }
            Mode::Lock { lock_now } => {
                let user = accounts::current_user().unwrap_or_default();
                self.username = user.clone();
                ui.set_username(user.into());
                self.handle.on_lock_event(|e| with(|a| a.on_lock_event(e)));
                // cosmic-greeter's rules (`locker.rs:712-727`): logind reachable ⇒ wait for its
                // `Lock`, re-locking at once if the session was already locked (its lockfile; here
                // logind's own `LockedHint`, which this program sets); logind unreachable ⇒ lock
                // immediately. A session lookup that fails is a warning, never a lock (`logind.rs:
                // 91-93` warns and returns) — a unit that locked every login because it could not
                // find its session would be the worst possible failure mode.
                let (logind_ok, watching, was_locked) = match &self.logind {
                    Some(l) => {
                        let watching = l.has_session()
                            && l.watch_session(|s| {
                                let _ = slint::invoke_from_event_loop(move || with(|a| a.on_logind(s)));
                            })
                            .is_ok();
                        if !watching {
                            tracing::warn!("lock mode: no logind session to watch; resident, Lock signals will not arrive");
                        }
                        (true, watching, l.locked_hint())
                    }
                    None => (false, false, false),
                };
                if lock_now || !logind_ok || was_locked {
                    tracing::info!(lock_now, logind_ok, was_locked, "lock mode: locking now");
                    self.lock();
                } else {
                    tracing::info!(watching, "lock mode: resident, waiting for logind's Lock");
                }
            }
        }
    }

    fn tick_clock(&mut self) {
        let mut t = libc::tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: std::ptr::null() };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
        // SAFETY: localtime_r writes into our tm
        unsafe { libc::localtime_r(&now, &mut t) };
        let s = format!("{:02}:{:02}", t.tm_hour, t.tm_min);
        if self.ui.get_clock().as_str() != s {
            self.ui.set_clock(s.into());
        }
    }

    // ---- the conversation ----

    fn select_account(&mut self, name: String) {
        self.ui.set_username(name.clone().into());
        self.submit_username(name);
    }

    fn submit_username(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || self.backend.is_some() {
            return;
        }
        self.username = name;
        self.begin_conversation();
    }

    fn begin_conversation(&mut self) {
        let ui = &self.ui;
        ui.set_info_text("".into());
        ui.set_error_text("".into());
        ui.set_prompt_visible(false);
        ui.set_response("".into());
        ui.set_conversing(true);
        ui.set_busy(true);
        let (backend, rx) = Backend::new();
        self.backend = Some(backend);
        let user = self.username.clone();
        match self.mode {
            Mode::Greeter => {
                let idx = (self.ui.get_session_index().max(0) as usize).min(self.sessions.len().saturating_sub(1));
                let cmd = self.sessions.get(idx).map(|s| s.cmd.clone()).unwrap_or_else(|| vec!["mura-session".into(), "start".into()]);
                let session = crate::greetd::Session { cmd, env: crate::sessions::env() };
                std::thread::Builder::new().name("greetd".into()).spawn(move || crate::greetd::run(user, session, rx)).expect("thread");
            }
            Mode::Lock { .. } => {
                std::thread::Builder::new().name("authd".into()).spawn(move || crate::authd::run(user, rx)).expect("thread");
            }
        }
    }

    fn respond(&mut self, mut text: String) {
        let Some(b) = &self.backend else { return };
        if !self.ui.get_prompt_visible() {
            return;
        }
        b.respond(text.clone());
        conv::zeroize(&mut text);
        self.ui.set_response("".into());
        self.ui.set_prompt_visible(false);
        self.ui.set_busy(true);
    }

    fn cancel(&mut self) {
        if let Some(b) = self.backend.take() {
            b.cancel();
        }
        self.ui.set_response("".into());
        match self.mode {
            Mode::Greeter => {
                self.ui.set_conversing(false);
                self.ui.set_prompt_visible(false);
                self.ui.set_busy(false);
            }
            // nothing to go back to on a locked screen: the conversation starts over
            Mode::Lock { .. } => self.begin_conversation(),
        }
    }

    pub fn on_conversation_event(&mut self, event: Event) {
        let ui = self.ui.clone_strong();
        match event {
            Event::Prompt { text, secret } => {
                let uid = self.accounts.iter().find(|a| a.name == self.username).map(|a| a.uid);
                let pad = secret && accounts::numeric_hint(&self.username, uid);
                ui.set_prompt_text(text.into());
                ui.set_prompt_secret(secret);
                ui.set_digit_pad(pad);
                ui.set_response("".into());
                ui.set_prompt_visible(true);
                ui.set_busy(false);
            }
            Event::Info(t) => ui.set_info_text(t.into()),
            Event::Error(t) => ui.set_error_text(t.into()),
            Event::Success => {
                self.backend = None;
                ui.set_busy(false);
                ui.set_prompt_visible(false);
                match self.mode {
                    Mode::Greeter => {
                        accounts::remember_last_user(&self.username);
                        tracing::info!(user = %self.username, "session starting; exiting (greetd runs it when the greeter is gone)");
                        let _ = slint::quit_event_loop();
                    }
                    Mode::Lock { lock_now } => {
                        tracing::info!("unlocked");
                        self.handle.unlock();
                        if let Some(l) = &self.logind {
                            l.set_locked_hint(false);
                        }
                        ui.set_conversing(false);
                        ui.set_error_text("".into());
                        ui.set_info_text("".into());
                        if lock_now {
                            // the harness path: one lock, then exit
                            let _ = slint::quit_event_loop();
                        }
                    }
                }
            }
            Event::Failure { delay_ms } => {
                self.backend = None;
                ui.set_prompt_visible(false);
                ui.set_error_text(GENERIC_FAILURE.into());
                ui.set_busy(true);
                // the fail delay is honoured before the retry UI (session-auth §2.3); then the
                // conversation is re-created for the same user (tuigreet's soft reset)
                let wait = Duration::from_millis(delay_ms.max(300));
                self.retry_timer.start(slint::TimerMode::SingleShot, wait, || {
                    with(|a| {
                        if a.backend.is_none() && (matches!(a.mode, Mode::Lock { .. }) || a.ui.get_conversing()) {
                            let err = a.ui.get_error_text();
                            a.begin_conversation();
                            a.ui.set_error_text(err);
                        }
                    })
                });
            }
        }
    }

    // ---- lock mode ----

    fn lock(&mut self) {
        self.ui.set_busy(true);
        self.handle.lock();
    }

    fn on_lock_event(&mut self, e: mura_slint_platform::LockEvent) {
        match e {
            mura_slint_platform::LockEvent::Locked => {
                // I2: zxr composed the blank frame and said `locked`; tell logind, then converse
                if let Some(l) = &self.logind {
                    l.set_locked_hint(true);
                }
                if self.backend.is_none() {
                    self.begin_conversation();
                }
            }
            mura_slint_platform::LockEvent::Finished => {
                tracing::warn!("lock finished by the compositor (another locker, or refused); staying resident");
                self.ui.set_busy(false);
                if let Mode::Lock { lock_now: true } = self.mode {
                    let _ = slint::quit_event_loop();
                }
            }
        }
    }

    fn on_logind(&mut self, s: Signal) {
        match s {
            Signal::Lock => {
                if !self.handle.is_locked() {
                    tracing::info!("logind: Lock");
                    self.lock();
                }
            }
            Signal::Unlock => {
                // an administrator's `loginctl unlock-session`: logind's word is final here
                tracing::info!("logind: Unlock");
                if let Some(b) = self.backend.take() {
                    b.cancel();
                }
                self.handle.unlock();
                self.ui.set_conversing(false);
                self.ui.set_busy(false);
            }
        }
    }

    // ---- power ----

    fn power_requested(&mut self, kind: &str) {
        let Some(p) = Power::parse(kind) else { return };
        self.pending_power = Some(p);
        self.ui.set_power_pending(kind.into());
        self.ui.set_power_countdown(POWER_COUNTDOWN_S);
        self.power_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
            with(|a| {
                let n = a.ui.get_power_countdown() - 1;
                a.ui.set_power_countdown(n);
                if n <= 0 {
                    a.power_confirmed();
                }
            })
        });
    }

    fn power_confirmed(&mut self) {
        self.power_timer.stop();
        self.ui.set_power_pending("".into());
        if let (Some(p), Some(l)) = (self.pending_power.take(), &self.logind) {
            if let Err(e) = l.power(p) {
                tracing::error!(?p, "logind: {e}");
                self.ui.set_error_text("Not permitted".into());
            }
        }
    }

    fn power_cancelled(&mut self) {
        self.power_timer.stop();
        self.pending_power = None;
        self.ui.set_power_pending("".into());
    }

    // ---- accessibility: the input floor is zxr's; the toggle is a control-socket line ----

    fn toggle_dwell(&mut self, on: bool) {
        let Some(path) = &self.zxr_control else { return };
        use std::io::Write;
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(mut s) => {
                let _ = s.write_all(format!("a11y dwell {}\n", if on { "on" } else { "off" }).as_bytes());
            }
            Err(e) => tracing::warn!(path, "zxr control: {e}"),
        }
    }
}
