//! logind over D-Bus (research/78 §5 det. 5, §6): power actions gated by `Can*` — cosmic-greeter's
//! `greeter.rs`, SDDM's daemon, LightDM's `power.c`; `allow_active` is what lets a seat session do
//! it without a helper — and, in lock mode, the session's `Lock`/`Unlock` signals and
//! `SetLockedHint` (cosmic-greeter `src/logind.rs:94-139`; the hint is I2's report to logind
//! after zxr's `locked`). Blocking zbus: calls are milliseconds and run on the UI thread; the
//! signal wait is one thread parked on the bus — zero wake-ups while idle.

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

pub struct Logind {
    conn: Connection,
    manager: Proxy<'static>,
    session: Option<Proxy<'static>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    PowerOff,
    Reboot,
    Suspend,
}

impl Power {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "poweroff" => Some(Power::PowerOff),
            "reboot" => Some(Power::Reboot),
            "suspend" => Some(Power::Suspend),
            _ => None,
        }
    }
    fn method(self) -> (&'static str, &'static str) {
        match self {
            Power::PowerOff => ("CanPowerOff", "PowerOff"),
            Power::Reboot => ("CanReboot", "Reboot"),
            Power::Suspend => ("CanSuspend", "Suspend"),
        }
    }
}

/// A `Lock`/`Unlock` from logind, posted to the UI thread.
#[derive(Clone, Copy, Debug)]
pub enum Signal {
    Lock,
    Unlock,
}

impl Logind {
    pub fn connect() -> zbus::Result<Self> {
        let conn = Connection::system()?;
        let manager = Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;
        let session = Self::find_session(&conn, &manager).ok();
        Ok(Logind { conn, manager, session })
    }

    fn find_session(conn: &Connection, manager: &Proxy<'static>) -> zbus::Result<Proxy<'static>> {
        let path: OwnedObjectPath = match std::env::var("XDG_SESSION_ID") {
            Ok(id) => manager.call("GetSession", &(id,))?,
            Err(_) => manager.call("GetSessionByPID", &(0u32,))?,
        };
        Proxy::new(conn, "org.freedesktop.login1", path, "org.freedesktop.login1.Session")
    }

    /// `Can*` says "yes" (not "challenge": the greeter/lock runs without an authentication agent).
    pub fn can(&self, p: Power) -> bool {
        let (can, _) = p.method();
        matches!(self.manager.call::<_, _, String>(can, &()).as_deref(), Ok("yes"))
    }

    pub fn power(&self, p: Power) -> zbus::Result<()> {
        let (_, method) = p.method();
        self.manager.call::<_, _, ()>(method, &(true,))
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    pub fn set_locked_hint(&self, locked: bool) {
        if let Some(s) = &self.session {
            if let Err(e) = s.call::<_, _, ()>("SetLockedHint", &(locked,)) {
                tracing::warn!("SetLockedHint({locked}): {e}");
            }
        }
    }

    /// Park a thread on the session's `Lock`/`Unlock` signals; each is posted with `deliver`.
    pub fn watch_session(&self, deliver: impl Fn(Signal) + Send + 'static) -> zbus::Result<()> {
        let Some(session) = &self.session else {
            return Err(zbus::Error::Failure("no logind session".into()));
        };
        let session = session.clone();
        let _conn = self.conn.clone();
        std::thread::Builder::new()
            .name("logind-signals".into())
            .spawn(move || {
                let iter = match session.receive_all_signals() {
                    Ok(i) => i,
                    Err(e) => {
                        tracing::error!("logind signals: {e}");
                        return;
                    }
                };
                for msg in iter {
                    let header = msg.header();
                    match header.member().map(|m| m.as_str()) {
                        Some("Lock") => deliver(Signal::Lock),
                        Some("Unlock") => deliver(Signal::Unlock),
                        _ => {}
                    }
                }
            })
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        Ok(())
    }
}
