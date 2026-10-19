// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The native skills installer lands the right files, and needs no shell.
//!
//! These drive the REAL `tina4` binary against REAL fixtures served over
//! `file://` (curl reads them locally, so there is no network and no mock).
//! The first test proves the eight skills and their references land in the
//! target directory with the ref marker, all gated by a real `skills.sha256`.
//! The second is the regression the whole change exists for: the old installer
//! ran `sh install-skills.sh` (or `powershell -c "iex(...)"`), so it could not
//! work without a shell on the PATH. The native installer must, so this runs it
//! with a PATH that has `curl` and nothing else — no `sh`, no `bash`, no
//! `powershell`. Against the old shell-out this fails; against the port it
//! succeeds.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Guarantees a unique directory name even when two parallel tests call
/// `private_dir` with the same label in the same nanosecond (the timestamp alone
/// is not enough - both tests build the "fixtures" dir, and a coincident clock
/// read gave them the same name and an AlreadyExists collision).
static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

/// The ref the fixtures live under. A test-only value, so the run never depends
/// on the installer's shipped default.
const TEST_REF: &str = "0.0.0-test";

/// Every skill and its `references/` files — the same set the installer stages
/// (mirrors `INSTALLS` in src/skills.rs). A file here the installer does not
/// fetch, or the reverse, breaks the checksum step, which is the point.
const DEV_REFS: &[&str] = &[
    "auth-and-services.md",
    "data-and-orm.md",
    "deployment.md",
    "routes-and-api.md",
    "templates-and-frontend.md",
    "realtime.md",
    "web-push.md",
    "ai-coder-rule-path.svg",
];

fn installs() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("tina4-developer-python", DEV_REFS),
        ("tina4-developer-php", DEV_REFS),
        ("tina4-developer-ruby", DEV_REFS),
        ("tina4-developer-nodejs", DEV_REFS),
        ("tina4-js", &["html-and-components.md", "signals-and-reactivity.md", "persistence.md", "rtc.md"]),
        ("tina4-maintainer", &["cli-and-deployment.md", "frond-and-frontend.md", "routing-and-orm.md", "subsystems.md"]),
        ("tina4-architect", &[]),
        ("tina4-design", &[]),
    ]
}

/// Every stage-relative file the installer stages, in the manifest's sorted order.
fn stage_relpaths() -> Vec<String> {
    let mut paths = Vec::new();
    for (skill, references) in installs() {
        paths.push(format!("{skill}/SKILL.md"));
        for reference in references {
            paths.push(format!("{skill}/references/{reference}"));
        }
    }
    paths
}

/// Deterministic content for a staged file, keyed only on its path.
fn fixture_bytes(relpath: &str) -> Vec<u8> {
    format!("tina4 skills fixture: {relpath}\n").into_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// A private 0700 temp directory that will not collide with another user's.
fn private_dir(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tina4-native-skills-{}-{label}-{stamp}-{seq}",
        std::process::id()
    ));
    fs::create_dir(&dir).expect("could not create a private temp directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("could not narrow it");
    dir
}

/// Lay out the fixture tree the `tina4` (flat) tier serves:
/// `<root>/<ref>/<skill>/<relative>` plus `<root>/<ref>/skills.sha256`.
/// Returns the root directory to hand to the installer as a `file://` root.
fn build_fixtures() -> PathBuf {
    let root = private_dir("fixtures");
    let ref_dir = root.join(TEST_REF);
    let mut manifest_lines: Vec<String> = Vec::new();
    for relpath in stage_relpaths() {
        let file = ref_dir.join(&relpath);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = fixture_bytes(&relpath);
        fs::write(&file, &bytes).unwrap();
        manifest_lines.push(format!("{}  {relpath}", sha256_hex(&bytes)));
    }
    manifest_lines.sort();
    fs::write(ref_dir.join("skills.sha256"), manifest_lines.join("\n") + "\n").unwrap();
    root
}

fn file_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}

/// Run `tina4 skills <target>` with the fixture roots, into `home`.
fn run_install(home: &Path, fixtures_url: &str, extra_path: Option<&Path>) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tina4"));
    cmd.args(["skills", "codex"])
        .env("HOME", home)
        .env("TINA4_SKILLS_HOME", home)
        .env("TINA4_SKILLS_REF", TEST_REF)
        .env("TINA4_SKILLS_TINA4_ROOT", fixtures_url)
        .env("TINA4_SKILLS_JSDELIVR_ROOT", fixtures_url)
        .env("TINA4_SKILLS_RAW_ROOT", fixtures_url)
        .env("TINA4_SKILLS_RETRY_DELAY", "0")
        .env("TINA4_SKILLS_RETRY_COUNT", "1");
    if let Some(path) = extra_path {
        cmd.env("PATH", path);
    }
    cmd.output().expect("could not run the tina4 binary")
}

fn assert_all_skills_landed(home: &Path) {
    let dest = home.join(".agents").join("skills");
    assert_eq!(
        fs::read_to_string(dest.join(".tina4-skills-ref")).unwrap().trim(),
        TEST_REF,
        "the ref marker was not written"
    );
    for (skill, references) in installs() {
        let skill_bytes = fixture_bytes(&format!("{skill}/SKILL.md"));
        assert_eq!(
            fs::read(dest.join(skill).join("SKILL.md")).unwrap(),
            skill_bytes,
            "SKILL.md wrong or missing for {skill}"
        );
        for reference in references {
            assert!(
                dest.join(skill).join("references").join(reference).is_file(),
                "missing reference {reference} for {skill}"
            );
        }
    }
    // The legacy skill dir must never be created by an install.
    assert!(!dest.join("tina4-developer").exists());
}

#[test]
fn the_native_installer_lands_every_skill_verified() {
    let fixtures = build_fixtures();
    let home = private_dir("home");
    let out = run_install(&home, &file_url(&fixtures), None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "install failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("verified"), "the checksum gate did not run:\n{stdout}");
    assert_all_skills_landed(&home);
    let _ = fs::remove_dir_all(&fixtures);
    let _ = fs::remove_dir_all(&home);
}

/// The regression proof: a PATH with `curl` and NO shell interpreter. The old
/// shell-out could not install here; the native port must.
#[test]
fn the_native_installer_needs_no_shell_on_path() {
    let fixtures = build_fixtures();
    let home = private_dir("home-noshell");
    let bin = private_dir("curl-only-bin");

    // SYMLINK the real curl in - do NOT copy it. On macOS /usr/bin/curl is a
    // code-signed platform binary; a COPY has no valid signature, so AMFI kills
    // it with SIGKILL (exit 137) the moment the installer spawns it, and the test
    // fails on every Mac for a reason that has nothing to do with the shell. A
    // symlink to the EXISTING real curl runs the signed binary unchanged and
    // works on Linux too. (The old concern - "a symlink to a MISSING target would
    // spawn-fail" - does not apply: real_curl exists.) Nothing else goes in: no
    // sh, no bash, no powershell.
    let real_curl = which_curl();
    std::os::unix::fs::symlink(&real_curl, bin.join("curl")).expect("could not stage curl");
    assert!(!bin.join("sh").exists() && !bin.join("bash").exists() && !bin.join("powershell").exists());

    let out = run_install(&home, &file_url(&fixtures), Some(&bin));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the native installer could not run without a shell on PATH:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_all_skills_landed(&home);

    let _ = fs::remove_dir_all(&fixtures);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&bin);
}

/// Find the real curl binary on the current PATH.
fn which_curl() -> PathBuf {
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let candidate = Path::new(dir).join("curl");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("no curl on PATH; the CLI's download primitive needs it");
}
