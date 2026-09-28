//! The conversation, as the scene sees it: one shape for both backends (research/78 §9 det. 3).
//! A backend runs on its own thread — greetd answers after PAM, which may sleep on a fail delay,
//! and the scene must keep rendering and taking dwell — and posts [`Event`]s to the UI thread
//! through `slint::invoke_from_event_loop`; the UI answers through [`Backend::respond`] /
//! [`Backend::cancel`].

use std::sync::mpsc::{channel, Receiver, Sender};

/// What a backend tells the scene.
#[derive(Debug, Clone)]
pub enum Event {
    /// A prompt awaiting a response (`visible`/`secret`), text verbatim.
    Prompt { text: String, secret: bool },
    /// `info`: shown inline, already acknowledged.
    Info(String),
    /// `error` from PAM: shown inline, already acknowledged; the conversation continues.
    Error(String),
    /// Authenticated (greeter mode: the session was also started).
    Success,
    /// The conversation ended in failure; the scene shows **generic** text and, after `delay_ms`,
    /// offers a retry (session-auth §2.3; greetd has no delay of its own).
    Failure { delay_ms: u64 },
}

/// What the scene tells a backend.
#[derive(Debug)]
pub enum Command {
    Respond(String),
    Cancel,
}

/// A running conversation's handle (the thread owns the socket).
pub struct Backend {
    tx: Sender<Command>,
}

impl Backend {
    pub fn new() -> (Self, Receiver<Command>) {
        let (tx, rx) = channel();
        (Backend { tx }, rx)
    }
    pub fn respond(&self, text: String) {
        let _ = self.tx.send(Command::Respond(text));
    }
    pub fn cancel(&self) {
        let _ = self.tx.send(Command::Cancel);
    }
}

/// The thread → UI channel: `slint::invoke_from_event_loop` with the app's thread-local.
pub fn post(event: Event) {
    let _ = slint::invoke_from_event_loop(move || crate::app::with(|app| app.on_conversation_event(event)));
}

/// Zero a secret before it drops (the helper does the same on its side).
pub fn zeroize(s: &mut String) {
    // SAFETY: overwriting the bytes of an owned String with zeros keeps it valid UTF-8.
    unsafe {
        for b in s.as_bytes_mut() {
            std::ptr::write_volatile(b, 0);
        }
    }
    s.clear();
}
