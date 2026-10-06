//! `tina4 serve` must take a busy port over only from this project's own Tina4
//! dev server, never from whatever else happens to hold it.
//!
//! On 3.8.88 serve signalled every PID `lsof` listed for the port — an
//! unrelated `python3 -m http.server`, and even a client merely connected to
//! it. The framework ports already identify their own server by the PID file it
//! writes when it binds, `data/.tina4-serve-<port>.pid` (TAKEOVER-DEC-01); these
//! drive the shipped binary to check the CLI now makes the same call.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tina4-takeover-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A separate process listening on `port`, so a wrong kill is observable and
/// cannot take the test harness down with it.
fn holder(port: u16) -> Option<Child> {
    let python = ["python3", "python"]
        .into_iter()
        .find(|p| Command::new(p).arg("-V").output().is_ok())?;
    let child = Command::new(python)
        .args([
            "-c",
            &format!(
                "import socket,time\ns=socket.socket()\ns.bind(('127.0.0.1',{}))\ns.listen()\ntime.sleep(60)",
                port
            ),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
        assert!(Instant::now() < deadline, "the holder never bound {}", port);
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(child)
}

fn alive(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

/// A Python project whose "server" is a stub that records it was started and
/// exits, so serve returns once it gets past the port.
fn project(dir: &Path, env_file: Option<&str>) -> PathBuf {
    fs::write(dir.join("requirements.txt"), "tina4_python\n").unwrap();
    fs::write(dir.join("app.py"), "").unwrap();
    if let Some(contents) = env_file {
        fs::write(dir.join(".env"), contents).unwrap();
    }
    let bin = dir.join("stub-bin");
    fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("python3");
    let marker = dir.join("started");
    fs::write(&stub, format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    marker
}

fn serve(dir: &Path, args: &[&str]) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .arg("serve")
        .args(args)
        .current_dir(dir)
        .env_clear()
        // /usr/sbin + /sbin so the spawned serve finds `lsof` on macOS (it lives
        // in /usr/sbin there, not /usr/bin as on Linux); without it port_listeners
        // comes back empty and the identity-path tests can't run.
        .env("PATH", format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", dir.join("stub-bin").display()))
        .env("HOME", dir)
        .env("TINA4_NO_BROWSER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tina4 serve");
    // A blown cap is a named failure, never a wedged run.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string()
        + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn a_foreign_holder_of_an_explicit_port_is_left_running() {
    let dir = temp_dir("foreign-explicit");
    let port = free_port();
    let Some(mut other) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    let started = project(&dir, None);

    let (code, text) = serve(&dir, &["--port", &port.to_string()]);

    assert!(alive(&mut other), "serve killed a process that is not Tina4's:\n{}", text);
    assert_ne!(code, 0, "a refused port must be visible to the caller:\n{}", text);
    assert!(text.contains(&other.id().to_string()), "name the holder:\n{}", text);
    assert!(!started.exists(), "serve started anyway on a port it does not have");
    let _ = other.kill();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_foreign_holder_of_the_default_port_is_left_running() {
    let dir = temp_dir("foreign-default");
    let port = free_port();
    let Some(mut other) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    project(&dir, Some(&format!("TINA4_PORT={}\n", port)));

    let (code, text) = serve(&dir, &[]);

    assert!(alive(&mut other), "serve killed a process that is not Tina4's:\n{}", text);
    assert_ne!(code, 0, "{}", text);
    let _ = other.kill();
    let _ = fs::remove_dir_all(&dir);
}

/// A record left by a crashed server names some other PID: still not ours.
#[test]
fn a_pid_file_naming_another_process_does_not_make_the_holder_ours() {
    let dir = temp_dir("stale-record");
    let port = free_port();
    let Some(mut other) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    project(&dir, None);
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(dir.join("data").join(format!(".tina4-serve-{}.pid", port)), "999999\n").unwrap();

    let (_, text) = serve(&dir, &["--port", &port.to_string()]);

    assert!(alive(&mut other), "a stale record must not license a kill:\n{}", text);
    let _ = other.kill();
    let _ = fs::remove_dir_all(&dir);
}

/// The convenience the takeover exists for must survive: this project's own
/// dev server, identified by its PID file, is still reclaimed.
#[test]
fn this_projects_own_dev_server_is_still_reclaimed() {
    let dir = temp_dir("own");
    let port = free_port();
    let Some(mut ours) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    let started = project(&dir, None);
    fs::create_dir_all(dir.join("data")).unwrap();
    let pidfile = dir.join("data").join(format!(".tina4-serve-{}.pid", port));
    fs::write(&pidfile, format!("{}\n", ours.id())).unwrap();

    let (_, text) = serve(&dir, &["--port", &port.to_string()]);

    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(&mut ours) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(&mut ours), "the project's own stale server was not reclaimed:\n{}", text);
    assert!(started.exists(), "serve did not go on to start the server:\n{}", text);
    assert!(!pidfile.exists(), "the reclaimed server's record was left behind");
    let _ = fs::remove_dir_all(&dir);
}

/// A client connected to the port is not its holder. `lsof` without a LISTEN
/// filter names both, and 3.8.88 killed both; the refusal must name only the
/// listener. The client here is this test process itself.
#[test]
fn a_connected_client_is_not_named_as_the_holder() {
    let dir = temp_dir("client");
    let port = free_port();
    let Some(mut other) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    let _conn = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to holder");
    project(&dir, None);

    let (_, text) = serve(&dir, &["--port", &port.to_string()]);

    let me = std::process::id().to_string();
    let named = text
        .lines()
        .find(|l| l.contains("is held by PID"))
        .unwrap_or_else(|| panic!("no refusal printed:\n{}", text));
    assert!(named.contains(&other.id().to_string()), "{}", named);
    assert!(
        !named.split(|c: char| !c.is_ascii_digit()).any(|t| t == me),
        "the connected client was named as a holder: {}",
        named
    );
    let _ = other.kill();
    let _ = fs::remove_dir_all(&dir);
}

/// TAKEOVER-DEC-03 opt-out: `--no-kill` refuses to reclaim the port even from
/// this project's OWN dev server. The pid file names the holder, so without the
/// gate the identity check would license a kill — the gate is what spares it.
#[test]
fn opt_out_leaves_even_our_own_server_running() {
    let dir = temp_dir("optout");
    let port = free_port();
    let Some(mut ours) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    project(&dir, None);
    fs::create_dir_all(dir.join("data")).unwrap();
    let pidfile = dir.join("data").join(format!(".tina4-serve-{}.pid", port));
    fs::write(&pidfile, format!("{}\n", ours.id())).unwrap();

    let (code, text) = serve(&dir, &["--port", &port.to_string(), "--no-kill"]);

    assert!(alive(&mut ours), "--no-kill killed our own server:\n{}", text);
    assert_ne!(code, 0, "an opted-out busy port must fail serve:\n{}", text);
    assert!(
        text.contains("opted out") || text.contains("no-kill") || text.contains("TINA4_NO_TAKEOVER"),
        "the refusal must name the opt-out:\n{}",
        text
    );
    let _ = ours.kill();
    let _ = fs::remove_dir_all(&dir);
}

/// TAKEOVER-DEC-03 dev gate: `--production` is a production bind and never
/// reclaims a port, even from this project's own dev server named in the pid
/// file. A production bind killing a port holder is exactly the surprise the
/// gate removes.
#[test]
fn production_leaves_even_our_own_server_running() {
    let dir = temp_dir("prod");
    let port = free_port();
    let Some(mut ours) = holder(port) else {
        eprintln!("skipped: no python to hold the port");
        return;
    };
    project(&dir, None);
    fs::create_dir_all(dir.join("data")).unwrap();
    let pidfile = dir.join("data").join(format!(".tina4-serve-{}.pid", port));
    fs::write(&pidfile, format!("{}\n", ours.id())).unwrap();

    let (code, text) = serve(&dir, &["--port", &port.to_string(), "--production"]);

    assert!(alive(&mut ours), "--production killed our own server:\n{}", text);
    assert_ne!(code, 0, "a production bind on a busy port must fail serve:\n{}", text);
    assert!(
        text.contains("outside dev mode") || text.contains("production"),
        "the refusal must name the dev gate:\n{}",
        text
    );
    let _ = ours.kill();
    let _ = fs::remove_dir_all(&dir);
}
