// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `.env` values must survive the CLI, through the real entry points.
//!
//! Both defects these cover were reported against 3.8.88 and reproduced there:
//!
//!   * `tina4 serve` (no `--port`) parses `.env` itself and exports it to the
//!     language server. `TINA4_CSP="default-src 'self'; form-action 'self'"`
//!     arrived as `... form-action 'self` — not a valid CSP source expression,
//!     so the browser dropped the directive and blocked the login form post.
//!   * `tina4 env --sync` read the same way and wrote values back bare, so the
//!     truncation landed in the user's own file and compounded on every run.
//!
//! These go through the shipped binary, not the helpers: the defect was in what
//! the entry points did with them.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// The value from the report, plus the shapes that decayed on a second `--sync`.
const ENV_FILE: &str = concat!(
    "TINA4_CSP=\"default-src 'self'; form-action 'self'\"\n",
    "PLAIN_TRAILING_SQ=form-action 'self'\n",
    "SQ_WRAPPED='hello world'\n",
    "DQ_WRAPPED=\"hello world\"\n",
    "ENDS_DQ='say \"hi\"'\n",
    "QUOTED_EMPTY=\"''\"\n",
    "PADDED=\" spaced \"\n",
);

fn expected() -> Vec<(&'static str, &'static str)> {
    vec![
        ("TINA4_CSP", "default-src 'self'; form-action 'self'"),
        ("PLAIN_TRAILING_SQ", "form-action 'self'"),
        ("SQ_WRAPPED", "hello world"),
        ("DQ_WRAPPED", "hello world"),
        ("ENDS_DQ", "say \"hi\""),
        ("QUOTED_EMPTY", "''"),
        ("PADDED", " spaced "),
    ]
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tina4-dotenv-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Parse a `.env` the way a consumer would, WITHOUT reusing the CLI's parser —
/// a test that shares the parser cannot see the parser be wrong.
fn read_back(path: &Path) -> Vec<(String, String)> {
    let contents = fs::read_to_string(path).expect("read .env");
    let mut out = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let unwrapped = if value.len() >= 2
                && (value.starts_with('"') || value.starts_with('\''))
                && value.ends_with(value.chars().next().unwrap())
            {
                &value[1..value.len() - 1]
            } else {
                value
            };
            out.push((key.trim().to_string(), unwrapped.to_string()));
        }
    }
    out
}

fn value_of<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn env_sync_leaves_every_value_exactly_as_it_found_it() {
    let dir = temp_dir("sync");
    let env_path = dir.join(".env");
    fs::write(&env_path, ENV_FILE).expect("seed .env");

    for run in 1..=2 {
        let output = Command::new(env!("CARGO_BIN_EXE_tina4"))
            .args(["env", "--sync"])
            .current_dir(&dir)
            .output()
            .expect("run tina4 env --sync");
        assert!(
            output.status.success(),
            "run {} failed: {}",
            run,
            String::from_utf8_lossy(&output.stderr)
        );

        let pairs = read_back(&env_path);
        for (key, want) in expected() {
            assert_eq!(
                value_of(&pairs, key),
                Some(want),
                "after --sync run {}, {} was not preserved.\nfile now:\n{}",
                run,
                key,
                fs::read_to_string(&env_path).unwrap_or_default()
            );
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

/// The language server inherits the CLI's process environment. A stand-in for it
/// dumps that environment, which is exactly what the framework would have read.
#[cfg(unix)]
#[test]
fn serve_without_a_port_flag_exports_values_unharmed() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("serve");
    let dump = dir.join("dump.txt");
    let bin_dir = dir.join("stub-bin");
    fs::create_dir_all(&bin_dir).expect("create stub bin dir");

    // `tina4 serve` on a Python project runs `python3 app.py --managed`.
    fs::write(dir.join("requirements.txt"), "tina4_python\n").expect("write requirements.txt");
    fs::write(dir.join("app.py"), "").expect("write app.py");
    fs::write(
        &env_path_of(&dir),
        format!("{}TINA4_PORT=8919\n", ENV_FILE),
    )
    .expect("seed .env");

    let stub = bin_dir.join("python3");
    fs::write(
        &stub,
        format!(
            "#!/bin/sh\nenv > '{}'\nexit 0\n",
            dump.display()
        ),
    )
    .expect("write stub python3");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .arg("serve")
        .current_dir(&dir)
        .env("PATH", path)
        .env("TINA4_NO_BROWSER", "true")
        // The values under test must come from .env, not from this process.
        .env_remove("TINA4_CSP")
        .env_remove("PLAIN_TRAILING_SQ")
        .env_remove("SQ_WRAPPED")
        .env_remove("DQ_WRAPPED")
        .env_remove("ENDS_DQ")
        .env_remove("QUOTED_EMPTY")
        .env_remove("PADDED")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn tina4 serve");

    // A blown cap is a named failure, never a wedged run.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dump.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        dump.exists(),
        "the stand-in server was never started within 30s, so nothing was measured"
    );

    let dumped = fs::read_to_string(&dump).expect("read dumped environment");
    for (key, want) in expected() {
        let line = dumped
            .lines()
            .find(|l| l.starts_with(&format!("{}=", key)))
            .unwrap_or_else(|| panic!("{} never reached the server at all", key));
        let got = &line[key.len() + 1..];
        assert_eq!(got, want, "{} reached the server altered", key);
    }

    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn env_path_of(dir: &Path) -> std::path::PathBuf {
    dir.join(".env")
}
