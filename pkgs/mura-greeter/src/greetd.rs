//! Backend A — greetd over `$GREETD_SOCK` (research/78 §1–§2, det. 1 and 3): agreety's loop
//! (`greetd/agreety/src/main.rs:39-98`) with the conversation rules the greeters converge on —
//! prompts verbatim, `info`/`error` acknowledged at once with an empty response (regreet's
//! fingerprint-then-password reason), `error{auth_error}` → `cancel_session` + generic text +
//! a fresh `create_session` for the same user (tuigreet's soft reset), never the description.
//! On `success` after the last response the program sends `start_session` and exits: greetd
//! starts the session when its greeter process (zxr, which exits with us) is gone.

use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;

use greetd_ipc::codec::SyncCodec;
use greetd_ipc::{AuthMessageType, ErrorType, Request, Response};

use crate::conv::{post, Command, Event};

pub struct Session {
    pub cmd: Vec<String>,
    pub env: Vec<String>,
}

/// Run one greetd conversation for `username` on this thread until it ends.
pub fn run(username: String, session: Session, rx: Receiver<Command>) {
    let Ok(path) = std::env::var("GREETD_SOCK") else {
        tracing::error!("GREETD_SOCK not set: not started by greetd");
        post(Event::Failure { delay_ms: 0 });
        return;
    };
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(path, "greetd socket: {e}");
            post(Event::Failure { delay_ms: 0 });
            return;
        }
    };
    let send = |stream: &mut UnixStream, req: Request| -> Option<Response> {
        if let Err(e) = req.write_to(stream) {
            tracing::error!("greetd write: {e}");
            return None;
        }
        match Response::read_from(stream) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::error!("greetd read: {e}");
                None
            }
        }
    };
    let mut next = send(&mut stream, Request::CreateSession { username: username.clone() });
    loop {
        let Some(resp) = next.take() else {
            post(Event::Failure { delay_ms: 0 });
            return;
        };
        match resp {
            Response::AuthMessage { auth_message_type, auth_message } => match auth_message_type {
                AuthMessageType::Info | AuthMessageType::Error => {
                    // shown inline, acknowledged now; the conversation goes on
                    post(if matches!(auth_message_type, AuthMessageType::Info) { Event::Info(auth_message) } else { Event::Error(auth_message) });
                    next = send(&mut stream, Request::PostAuthMessageResponse { response: None });
                }
                AuthMessageType::Visible | AuthMessageType::Secret => {
                    post(Event::Prompt { text: auth_message, secret: matches!(auth_message_type, AuthMessageType::Secret) });
                    match rx.recv() {
                        Ok(Command::Respond(mut text)) => {
                            next = send(&mut stream, Request::PostAuthMessageResponse { response: Some(text.clone()) });
                            crate::conv::zeroize(&mut text);
                        }
                        Ok(Command::Cancel) | Err(_) => {
                            let _ = send(&mut stream, Request::CancelSession);
                            post(Event::Failure { delay_ms: 0 });
                            return;
                        }
                    }
                }
            },
            Response::Success => {
                // authenticated: start the session; greetd runs it when we (and zxr) exit
                match send(&mut stream, Request::StartSession { cmd: session.cmd.clone(), env: session.env.clone() }) {
                    Some(Response::Success) => {
                        tracing::info!(user = %username, "start_session accepted");
                        post(Event::Success);
                    }
                    Some(Response::Error { error_type, description }) => {
                        tracing::error!(?error_type, description, "start_session refused");
                        post(Event::Failure { delay_ms: 0 });
                    }
                    other => {
                        tracing::error!(?other, "start_session: unexpected reply");
                        post(Event::Failure { delay_ms: 0 });
                    }
                }
                return;
            }
            Response::Error { error_type, description } => {
                // the description is greetd's/PAM's and never shown (tuigreet's leak argument)
                tracing::info!(?error_type, description, "greetd conversation failed");
                let _ = send(&mut stream, Request::CancelSession);
                post(Event::Failure { delay_ms: if matches!(error_type, ErrorType::AuthError) { 0 } else { 0 } });
                return;
            }
        }
    }
}
