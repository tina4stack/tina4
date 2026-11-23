// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! tina4#160: the skills install limits each download, not the whole run.
//!
//! The installer used to stop the moment 60 seconds had passed since it
//! started, whatever was happening, because its download primitive ran `curl`
//! with no timeout and the run-wide budget was the only thing that caught a host
//! that hangs. A healthy install of 99 files on an ordinary connection crossed
//! 60 seconds and failed near the end every time.
//!
//! These drive the REAL `tina4` binary against a REAL HTTP server on loopback
//! (no mock, no network). One server is slow but always answers; the other
//! accepts connections and never says a word.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TEST_REF: &str = "0.0.0-test";

static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tina4-skills-timeouts-{}-{label}-{stamp}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Read one request head and return its path.
fn read_request_path(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    let line = String::from_utf8_lossy(&head).lines().next()?.to_string();
    line.split_whitespace().nth(1).map(str::to_string)
}

/// Serve `files` (path -> body). Every request for a path under `/skills/`
/// waits `delay` before the first byte, the way a slow link does.
fn serve_slowly(files: HashMap<String, Vec<u8>>, delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let files = Arc::new(files);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files = Arc::clone(&files);
            std::thread::spawn(move || {
                let mut stream = stream;
                let Some(path) = read_request_path(&mut stream) else { return };
                if !path.ends_with("skills.sha256") {
                    std::thread::sleep(delay);
                }
                let response = match files.get(&path) {
                    Some(body) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(body);
                        r
                    }
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec(),
                };
                let _ = stream.write_all(&response);
            });
        }
    });
    format!("http://{addr}/skills")
}

/// Accept every connection, read the request, and never answer. The sockets
/// are kept alive for the life of the test process so the client sees silence,
/// not a reset.
fn serve_nothing() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let held: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let held = Arc::clone(&held);
            std::thread::spawn(move || {
                let mut stream = stream;
                let _ = read_request_path(&mut stream);
                held.lock().unwrap().push(stream);
            });
        }
    });
    format!("http://{addr}/skills")
}

/// A manifest of `count` skills with one SKILL.md each, laid out the way the
/// tina4 tier serves them: `<root>/<ref>/<skill>/SKILL.md`.
fn fixture_files(count: usize) -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    let mut manifest = Vec::new();
    for n in 0..count {
        let relative = format!("tina4-timeout-fixture-{n:03}/SKILL.md");
        let body = format!("fixture {relative}\n").into_bytes();
        manifest.push(format!("{}  {relative}", sha256_hex(&body)));
        files.insert(format!("/skills/{TEST_REF}/{relative}"), body);
    }
    manifest.sort();
    files.insert(
        format!("/skills/{TEST_REF}/skills.sha256"),
        (manifest.join("\n") + "\n").into_bytes(),
    );
    files
}

struct Run {
    finished: bool,
    success: bool,
    elapsed: Duration,
    output: String,
}

/// Run `tina4 skills codex` against `root`, killing it after `deadline`.
fn run_install(root: &str, home: &Path, stall_secs: u64, deadline: Duration) -> Run {
    let out_path = home.join("out.log");
    let out_file = std::fs::File::create(&out_path).unwrap();
    let err_file = out_file.try_clone().unwrap();
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .args(["skills", "codex"])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("TINA4_SKILLS_HOME", home)
        .env("TINA4_SKILLS_REF", TEST_REF)
        .env("TINA4_SKILLS_TINA4_ROOT", root)
        .env("TINA4_SKILLS_JSDELIVR_ROOT", root)
        .env("TINA4_SKILLS_RAW_ROOT", root)
        .env("TINA4_SKILLS_RETRY_DELAY", "0")
        .env("TINA4_SKILLS_RETRY_COUNT", "0")
        .env("TINA4_SKILLS_STALL_TIMEOUT", stall_secs.to_string())
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(out_file)
        .stderr(err_file)
        .spawn()
        .expect("could not run the tina4 binary");
    let (finished, success) = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break (true, status.success());
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break (false, false);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    Run {
        finished,
        success,
        elapsed: started.elapsed(),
        output: std::fs::read_to_string(&out_path).unwrap_or_default(),
    }
}

/// The regression in the issue: every file arrives, just not quickly. 66 files
/// at one second each is more than the old 60-second run-wide budget, so the old
/// installer gave up near the end with nothing installed. Each file is well
/// inside the per-download stall limit, so the install must complete.
#[test]
fn a_slow_install_that_keeps_making_progress_completes() {
    let files = fixture_files(66);
    let root = serve_slowly(files, Duration::from_secs(1));
    let home = temp_dir("slow");

    let run = run_install(&root, &home, 5, Duration::from_secs(150));

    assert!(run.finished, "install still running after {:?}:\n{}", run.elapsed, run.output);
    assert!(run.success, "a slow but healthy install failed:\n{}", run.output);
    assert!(
        run.output.contains("verified 66 skill files"),
        "the checksum gate did not see every file:\n{}",
        run.output
    );
    let dest = home.join(".agents").join("skills");
    for n in 0..66 {
        let skill = dest.join(format!("tina4-timeout-fixture-{n:03}")).join("SKILL.md");
        assert!(skill.is_file(), "{} was not installed", skill.display());
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// The reason the old budget existed: a host that accepts the connection and
/// then says nothing. With a per-download stall limit the install still fails,
/// promptly, nothing is installed, and the hint names a command that works
/// outside a Tina4 project.
#[test]
fn a_host_that_never_answers_fails_promptly_with_a_usable_hint() {
    let root = serve_nothing();
    let home = temp_dir("mute");

    let run = run_install(&root, &home, 2, Duration::from_secs(60));

    assert!(
        run.finished,
        "the install hung on a silent host for {:?} instead of timing out:\n{}",
        run.elapsed,
        run.output
    );
    assert!(!run.success, "an install with no reachable host reported success:\n{}", run.output);
    assert!(
        run.output.contains("tina4 skills codex"),
        "the failure hint must name a command that works anywhere:\n{}",
        run.output
    );
    assert!(
        !run.output.contains("run later: tina4 ai"),
        "the failure hint still points at `tina4 ai`, which needs a project:\n{}",
        run.output
    );
    assert!(
        !home.join(".agents").join("skills").join(".tina4-skills-ref").exists(),
        "a failed install published something"
    );
    let _ = std::fs::remove_dir_all(&home);
}
