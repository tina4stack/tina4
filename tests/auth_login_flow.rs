//! `tina4 login` / `register` drive the GitHub browser-handoff + poll handshake
//! against profile.tina4.com. These run the REAL `tina4` binary against a REAL
//! local HTTP server that implements the portal's `/api/cli/auth` contract - no
//! mock, a genuine socket speaking the genuine protocol. They live in their own
//! integration binary (own process) so a sibling module's test cannot clobber
//! HOME mid-handshake, and each spawns its own portal on an ephemeral port.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A scripted poll reply: an HTTP status plus a JSON body.
#[derive(Clone)]
struct Reply {
    status: u16,
    body: String,
}

struct Portal {
    base: String,
    last_mode: Arc<Mutex<String>>,
}

fn http_reply(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        410 => "Gone",
        _ => "OK",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Read ONE HTTP request off the socket: returns (path, body). Minimal on purpose
/// - just enough of HTTP/1.1 to serve the two JSON POSTs the CLI makes.
fn read_request(stream: &TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some((path, String::from_utf8_lossy(&body).to_string()))
}

/// Pull the `mode` value out of a `{"mode":"login"}` JSON body (no serde here).
fn mode_of(body: &str) -> Option<String> {
    let after = body.split("\"mode\"").nth(1)?; // :"login"}
    after.split('"').nth(1).map(|s| s.to_string())
}

/// Start the portal on an ephemeral port. `polls` is consumed in order; once it
/// runs out the portal keeps answering 202 (pending).
fn spawn_portal(polls: Vec<Reply>) -> Portal {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let last_mode = Arc::new(Mutex::new(String::new()));
    let lm = last_mode.clone();
    let polls = Arc::new(polls);
    let idx = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { continue };
            let Some((path, body)) = read_request(&stream) else { continue };
            let resp = if path.starts_with("/api/cli/auth/start") {
                if let Some(m) = mode_of(&body) {
                    *lm.lock().unwrap() = m;
                }
                http_reply(
                    200,
                    "{\"request_id\":\"req-1\",\"verification_uri\":\"http://127.0.0.1/cli?code=X\",\"user_code\":\"WXYZ-1234\",\"poll_interval\":1,\"expires_in\":30}",
                )
            } else if path.starts_with("/api/cli/auth/poll") {
                let i = idx.fetch_add(1, Ordering::SeqCst);
                match polls.get(i) {
                    Some(r) => http_reply(r.status, &r.body),
                    None => http_reply(202, "{}"),
                }
            } else {
                http_reply(200, "{}")
            };
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    Portal {
        base: format!("http://127.0.0.1:{port}"),
        last_mode,
    }
}

fn temp_home(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tina4-authflow-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn creds_path(home: &Path) -> PathBuf {
    home.join(".config").join("tina4").join("credentials.json")
}

/// Run the real `tina4 <cmd>` against the portal, with a bounded wait.
fn run_tina4(base: &str, home: &Path, cmd: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .arg(cmd)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()))
        .env("HOME", home)
        .env("TINA4_PROFILE_URL", base)
        .env("TINA4_NO_BROWSER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tina4");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string()
        + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn login_signs_in_and_writes_the_token_0600() {
    // The real handshake: start -> one "still pending" poll -> the token.
    let portal = spawn_portal(vec![
        Reply { status: 202, body: "{}".into() },
        Reply {
            status: 200,
            body: r#"{"token":"t4_live_abc","login":"ada","profile_url":"http://127.0.0.1/u/ada"}"#.into(),
        },
    ]);
    let home = temp_home("login");

    let (code, text) = run_tina4(&portal.base, &home, "login");

    assert_eq!(code, 0, "login should succeed:\n{text}");
    let path = creds_path(&home);
    let raw = std::fs::read_to_string(&path).expect("credentials.json must be written");
    assert!(raw.contains("t4_live_abc"), "the token must be persisted:\n{raw}");
    assert!(raw.contains("ada"), "the login must be persisted:\n{raw}");
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600, "the token file must not be world-readable");
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn register_sends_mode_register_and_signs_in() {
    let portal = spawn_portal(vec![Reply {
        status: 200,
        body: r#"{"token":"t4_reg","login":"grace","profile_url":"http://127.0.0.1/u/grace"}"#.into(),
    }]);
    let home = temp_home("register");

    let (code, text) = run_tina4(&portal.base, &home, "register");

    assert_eq!(code, 0, "register should succeed:\n{text}");
    assert_eq!(
        *portal.last_mode.lock().unwrap(),
        "register",
        "the CLI must send mode=register to /start"
    );
    assert!(creds_path(&home).exists(), "register must persist credentials");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn an_expired_sign_in_fails_and_stores_nothing() {
    // The portal reports the request expired (410). serve must surface it and
    // write no credentials.
    let portal = spawn_portal(vec![Reply {
        status: 410,
        body: r#"{"error":"expired"}"#.into(),
    }]);
    let home = temp_home("expired");

    let (code, text) = run_tina4(&portal.base, &home, "login");

    assert_ne!(code, 0, "an expired sign-in must fail:\n{text}");
    assert!(text.contains("expired"), "the error must name the expiry:\n{text}");
    assert!(
        !creds_path(&home).exists(),
        "a failed sign-in must store nothing"
    );
    let _ = std::fs::remove_dir_all(&home);
}
