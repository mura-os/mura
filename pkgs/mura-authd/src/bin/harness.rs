//! mura-authd-harness — a test-only compositor stand-in for specs/session-auth.md §6.
//!
//! It plays the compositor's half of the §2 protocol against a real mura-authd: creates the
//! SOCK_SEQPACKET socketpair, spawns the helper with `--fd 3` and `MURA_AUTHD_NONCE` set
//! (the legacy `--nonce HEX` argv path only in the `argv-nonce` scenario), and drives one
//! scenario. Exit 0 when the scenario's expectations hold. Scenarios:
//!
//!   basic          prompt_batch → respond with PASSWORD → expect success (or failure if --expect-fail)
//!   stale-nonce    respond first with a wrong nonce (must be ignored), then correctly → success
//!   cancel         on the first prompt send cancel → failure(abort), non-zero exit
//!   kill           SIGKILL the helper mid-prompt → EOF, no terminal message; a fresh
//!                  conversation with a fresh nonce then succeeds (§6 item 1)
//!   revoked        invalidate the nonce BEFORE reading the terminal success → the message is
//!                  ignored and the harness stays "locked" (§6 item 3, the compositor's rule)
//!   slow           the PAM stack sleeps (test module): the harness keeps ticking while it waits
//!                  and success arrives after the sleep (§6 item 2 — the helper is a separate
//!                  process, so the caller's loop never blocks on PAM)
//!   batched        a test module issues two prompts + one info in one callback → exactly one
//!                  prompt_batch with three prompts, one respond_batch (§6 item 7)
//!   oversize       a 64 KiB + 1 record → failure(internal)            (§2.1 framing, §6 item 9)
//!   truncated      a record larger than the helper's buffer (MSG_TRUNC) → failure(internal)
//!   empty          a zero-length record → failure(internal)
//!   badjson        invalid JSON → failure(internal)
//!   unknown-type   a well-formed record with an unknown "type" → failure(internal)
//!   nul-response   a response containing U+0000 → failure(internal), never an empty answer
//!   dup-index      two responses for one prompt index → failure(internal)
//!   bad-service    --service outside mura-lock[-*] → refused before pam_start (exit 2, no records)
//!   argv-nonce     the legacy --nonce argv path still works (with a warning) for one release
//!
//! The nonce reaches the helper as MURA_AUTHD_NONCE in its environment (never argv).

use serde_json::{json, Value};
use std::os::raw::{c_int, c_void};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Conv {
    fd: c_int,
    nonce: String,
    child: Child,
    valid: bool,
}

fn nonce() -> String {
    use std::io::Read;
    let mut b = [0u8; 8];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).expect("urandom");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn spawn(authd: &str, user: &str, service: &str) -> Conv {
    spawn_with(authd, user, service, false)
}

fn spawn_with(authd: &str, user: &str, service: &str, nonce_on_argv: bool) -> Conv {
    let mut fds = [0 as c_int; 2];
    assert_eq!(unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) }, 0);
    let (ours, theirs) = (fds[0], fds[1]);
    let n = nonce();
    let mut cmd = Command::new(authd);
    cmd.args(["--fd", "3", "--user", user, "--service", service]);
    if nonce_on_argv {
        cmd.args(["--nonce", &n]);
    } else {
        cmd.env("MURA_AUTHD_NONCE", &n);
    }
    cmd.stdin(Stdio::null());
    unsafe {
        cmd.pre_exec(move || {
            if libc::dup2(theirs, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().expect("spawn mura-authd");
    unsafe { libc::close(theirs) };
    Conv { fd: ours, nonce: n, child, valid: true }
}

fn send_raw(c: &Conv, data: &[u8]) {
    let n = unsafe { libc::send(c.fd, data.as_ptr() as *const c_void, data.len(), libc::MSG_NOSIGNAL) };
    assert_eq!(n, data.len() as isize, "send");
}

fn send(c: &Conv, v: &Value) {
    send_raw(c, &serde_json::to_vec(v).unwrap());
}

/// Common tail of the malformed-record scenarios: the helper must answer failure(internal)
/// and exit non-zero.
fn expect_internal(mut c: Conv, label: &str) -> bool {
    let fin = recv(&c, Duration::from_secs(90)).expect("terminal message");
    let code = wait_exit(&mut c);
    println!("{label}: {fin} exit={code}");
    fin["type"] == "failure" && fin["reason"] == "internal" && code != 0
}

/// One record, or None on EOF. Blocks up to `timeout`.
fn recv(c: &Conv, timeout: Duration) -> Option<Value> {
    let mut pfd = libc::pollfd { fd: c.fd, events: libc::POLLIN, revents: 0 };
    let r = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as c_int) };
    if r <= 0 {
        return None;
    }
    let mut buf = vec![0u8; 65 * 1024];
    let n = unsafe { libc::recv(c.fd, buf.as_mut_ptr() as *mut c_void, buf.len(), 0) };
    if n <= 0 {
        return None;
    }
    serde_json::from_slice(&buf[..n as usize]).ok()
}

fn respond(c: &Conv, batch: &Value, password: &str, nonce_override: Option<&str>) {
    let conversation = batch["conversation"].as_u64().unwrap();
    let prompts = batch["prompts"].as_array().unwrap();
    let responses: Vec<Value> = prompts
        .iter()
        .map(|p| {
            let idx = p["index"].as_u64().unwrap();
            match p["style"].as_str().unwrap() {
                "secret" | "visible" | "radio" => json!({"index": idx, "response": password}),
                _ => json!({"index": idx}),
            }
        })
        .collect();
    send(c, &json!({"type": "respond_batch", "nonce": nonce_override.unwrap_or(&c.nonce), "conversation": conversation, "responses": responses}));
}

fn wait_exit(c: &mut Conv) -> i32 {
    c.child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1)
}

fn main() {
    let mut authd = String::new();
    let mut user = String::new();
    let mut service = String::from("mura-lock");
    let mut password = String::new();
    let mut scenario = String::from("basic");
    let mut expect_fail = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--authd" => authd = args.next().unwrap(),
            "--user" => user = args.next().unwrap(),
            "--service" => service = args.next().unwrap(),
            "--password" => password = args.next().unwrap(),
            "--scenario" => scenario = args.next().unwrap(),
            "--expect-fail" => expect_fail = true,
            _ => panic!("unknown arg {a}"),
        }
    }
    let t = Duration::from_secs(90);
    let ok = match scenario.as_str() {
        "basic" => {
            let mut c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            assert_eq!(batch["type"], "prompt_batch", "{batch}");
            assert_eq!(batch["prompts"][0]["style"], "secret", "first prompt is the secret");
            respond(&c, &batch, &password, None);
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            println!("terminal: {fin} exit={code}");
            if expect_fail {
                fin["type"] == "failure" && fin["reason"] == "auth" && code != 0
            } else {
                fin["type"] == "success" && fin["nonce"] == c.nonce && code == 0
            }
        }
        "stale-nonce" => {
            let mut c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            respond(&c, &batch, "definitely-wrong", Some("0000000000000000"));
            // the helper must still be waiting: nothing arrives within 3 s
            assert!(recv(&c, Duration::from_secs(3)).is_none(), "stale-nonce response was acted on");
            respond(&c, &batch, &password, None);
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            println!("terminal: {fin} exit={code}");
            fin["type"] == "success" && code == 0
        }
        "cancel" => {
            let mut c = spawn(&authd, &user, &service);
            let _batch = recv(&c, t).expect("prompt_batch");
            send(&c, &json!({"type": "cancel", "nonce": c.nonce}));
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            println!("terminal: {fin} exit={code}");
            fin["type"] == "failure" && fin["reason"] == "abort" && code != 0
        }
        "kill" => {
            let mut c = spawn(&authd, &user, &service);
            let _batch = recv(&c, t).expect("prompt_batch");
            c.child.kill().unwrap();
            let code = wait_exit(&mut c);
            let after = recv(&c, Duration::from_secs(3));
            println!("killed mid-prompt: exit={code} after={after:?}");
            let dead_ok = after.is_none() && code != 0; // EOF, no terminal message ⇒ failure(internal) on the compositor side
            // fresh conversation, fresh nonce
            let mut c2 = spawn(&authd, &user, &service);
            let batch = recv(&c2, t).expect("prompt_batch 2");
            assert_ne!(c.nonce, c2.nonce);
            respond(&c2, &batch, &password, None);
            let fin = recv(&c2, t).expect("terminal 2");
            let code2 = wait_exit(&mut c2);
            println!("retry: {fin} exit={code2}");
            dead_ok && fin["type"] == "success" && code2 == 0
        }
        "revoked" => {
            let mut c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            respond(&c, &batch, &password, None);
            // grace expired before the helper answered: the compositor invalidates the nonce…
            c.valid = false;
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            // …and a success bearing that nonce is ignored — the state stays locked (I3).
            let would_unlock = c.valid && fin["type"] == "success" && fin["nonce"] == c.nonce;
            println!("terminal: {fin} exit={code} nonce_valid={} would_unlock={would_unlock} (demonstrates the COMPOSITOR's rule; the helper's part is that only a nonce-matching respond_batch advanced it)", c.valid);
            fin["type"] == "success" && code == 0 && !would_unlock
        }
        "slow" => {
            // the test module sleeps 5 s inside pam_authenticate BEFORE pam_unix prompts: the
            // caller's loop must keep ticking the whole time the helper is inside PAM
            let start = Instant::now();
            let mut c = spawn(&authd, &user, &service);
            let mut ticks = 0u32;
            let tick_until = |c: &Conv, ticks: &mut u32| -> Value {
                loop {
                    if let Some(v) = recv(c, Duration::from_millis(200)) {
                        return v;
                    }
                    *ticks += 1;
                    if start.elapsed() > t {
                        panic!("helper silent past the timeout");
                    }
                }
            };
            let batch = tick_until(&c, &mut ticks);
            assert_eq!(batch["type"], "prompt_batch", "{batch}");
            respond(&c, &batch, &password, None);
            let fin = tick_until(&c, &mut ticks);
            let code = wait_exit(&mut c);
            println!("terminal after {:?} with {ticks} ticks: {fin} exit={code}", start.elapsed());
            fin["type"] == "success" && code == 0 && start.elapsed() >= Duration::from_secs(4) && ticks >= 10
        }
        "batched" => {
            let mut c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            let prompts = batch["prompts"].as_array().unwrap().clone();
            println!("batch: {batch}");
            let styles: Vec<&str> = prompts.iter().map(|p| p["style"].as_str().unwrap()).collect();
            let shape_ok = styles == ["secret", "visible", "info"];
            respond(&c, &batch, &password, None);
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            println!("terminal: {fin} exit={code}");
            shape_ok && fin["type"] == "success" && code == 0
        }
        "oversize" => {
            let c = spawn(&authd, &user, &service);
            let _ = recv(&c, t).expect("prompt_batch");
            send_raw(&c, &vec![b'{'; 64 * 1024 + 1]);
            expect_internal(c, "oversize")
        }
        "truncated" => {
            let c = spawn(&authd, &user, &service);
            let _ = recv(&c, t).expect("prompt_batch");
            // larger than the helper's receive buffer (64 KiB + 1): recvmsg sets MSG_TRUNC
            send_raw(&c, &vec![b' '; 100 * 1024]);
            expect_internal(c, "truncated")
        }
        "empty" => {
            let c = spawn(&authd, &user, &service);
            let _ = recv(&c, t).expect("prompt_batch");
            send_raw(&c, b""); // a zero-length seqpacket record
            expect_internal(c, "empty")
        }
        "badjson" => {
            let c = spawn(&authd, &user, &service);
            let _ = recv(&c, t).expect("prompt_batch");
            send_raw(&c, b"{\"type\": \"respond_batch\", ");
            expect_internal(c, "badjson")
        }
        "unknown-type" => {
            let c = spawn(&authd, &user, &service);
            let _ = recv(&c, t).expect("prompt_batch");
            send(&c, &json!({"type": "hello", "nonce": c.nonce}));
            expect_internal(c, "unknown-type")
        }
        "nul-response" => {
            let c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            let conversation = batch["conversation"].as_u64().unwrap();
            send(&c, &json!({"type": "respond_batch", "nonce": c.nonce, "conversation": conversation,
                             "responses": [{"index": 0, "response": "s3c\u{0000}ret"}]}));
            expect_internal(c, "nul-response")
        }
        "dup-index" => {
            let c = spawn(&authd, &user, &service);
            let batch = recv(&c, t).expect("prompt_batch");
            let conversation = batch["conversation"].as_u64().unwrap();
            send(&c, &json!({"type": "respond_batch", "nonce": c.nonce, "conversation": conversation,
                             "responses": [{"index": 0, "response": &password}, {"index": 0, "response": "again"}]}));
            expect_internal(c, "dup-index")
        }
        "bad-service" => {
            let mut c = spawn(&authd, &user, "system-login");
            let first = recv(&c, Duration::from_secs(10));
            let code = wait_exit(&mut c);
            println!("bad-service: first={first:?} exit={code}");
            first.is_none() && code == 2
        }
        "argv-nonce" => {
            let mut c = spawn_with(&authd, &user, &service, true);
            let batch = recv(&c, t).expect("prompt_batch");
            respond(&c, &batch, &password, None);
            let fin = recv(&c, t).expect("terminal message");
            let code = wait_exit(&mut c);
            println!("argv-nonce (legacy): {fin} exit={code}");
            fin["type"] == "success" && code == 0
        }
        other => panic!("unknown scenario {other}"),
    };
    println!("scenario {scenario}: {}", if ok { "PASS" } else { "FAIL" });
    std::process::exit(if ok { 0 } else { 1 });
}
