//! The skills refresh has to say why it failed.
//!
//! A report arrived showing `Installing tina4 AI skills for all...`, then the
//! skip line, and nothing in between -- no cause, on the one platform nobody
//! here can run. The cause had been in hand and thrown away:
//! `.map(|s| s.success()).unwrap_or(false)` mapped a failed spawn and a
//! non-zero exit onto the same bare `false`, so the caller had nothing to print.
//!
//! These drive the real binary through `install_skills_target`'s failure paths
//! and assert the reason reaches stdout. They gate the CALL SITE: every unit
//! test in `setup.rs` stays green if the `println!` is deleted.
//!
//! ## Why the assertions are shaped the way they are
//!
//! An earlier version asserted on `"could not be started"` and `"sh"`. Both are
//! produced by the PRE-EXISTING download branch as well -- `main.rs` prints
//! "the downloader could not be started", and the source URLs end
//! `install-skills.sh`. On a box where the download branch was reached instead,
//! the test passed against completely unfixed source. Every assertion here is
//! now a string only the repaired path can emit, and each test also asserts the
//! download branch was NOT the one taken.
//!
//! Hermetic by construction: `PATH` is a directory holding a `curl` that is a
//! copy of `true`, so the download "succeeds" without a network and without
//! writing a file. What the tests vary is whether the interpreter that would
//! run the installer can be spawned.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A private directory, 0700, that cannot collide with another user's.
/// `create_dir` fails rather than adopting a path someone else planted --
/// the same shape `skills_stage_dir` uses in production, and for the same
/// reason: a shell is about to be pointed at the contents.
fn private_dir(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "tina4-spawn-gate-{}-{label}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&dir).expect("could not create a private temp directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("could not narrow it");
    dir
}

/// A PATH directory holding exactly the programs named, plus a `curl` that
/// exits 0 without writing anything so no test here touches a network.
///
/// `curl` is COPIED, not symlinked: a symlink to a missing target is created
/// happily and then fails to spawn, which silently moved the run into the
/// download branch and made these tests pass against unfixed source.
fn farm(label: &str, programs: &[&str]) -> PathBuf {
    let dir = private_dir(label);
    let truth = which::which("true").expect("no `true` on this box, so `curl` cannot be stubbed");
    fs::copy(&truth, dir.join("curl")).expect("could not stub curl");
    assert!(dir.join("curl").exists(), "the curl stub did not land");
    for program in programs {
        let real = which::which(program).unwrap_or_else(|_| panic!("no {program} on this box"));
        std::os::unix::fs::symlink(real, dir.join(program)).expect("could not plant a program");
    }
    dir
}

fn refresh_skills_with(path: &Path, label: &str) -> String {
    let home = private_dir(&format!("home-{label}"));
    let out = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .args(["skills", "all"])
        .env("PATH", path)
        .env("HOME", &home)
        .output()
        .expect("could not run the tina4 binary");
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(path);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    // Both tests are about the SPAWN. If the run died in the download branch
    // instead, nothing below tests what it claims to.
    assert!(
        !stdout.contains("Could not download the skills installer"),
        "the run never reached the spawn -- it failed while downloading:\n{stdout}"
    );
    assert!(
        stdout.contains("Skills install skipped"),
        "the refresh did not fail at all, so this test proves nothing:\n{stdout}"
    );
    stdout
}

/// The reported failure: the interpreter cannot be spawned at all.
#[test]
fn a_spawn_that_never_starts_says_so() {
    let out = refresh_skills_with(&farm("nostart", &[]), "nostart");
    // Program-qualified on purpose. A bare "could not be started" is also what
    // the download branch prints, as "the downloader could not be started".
    assert!(
        out.contains("sh could not be started"),
        "a refresh that died at the spawn still printed no cause:\n{out}"
    );
    assert!(
        !out.contains("ran and exited"),
        "a spawn that never started was reported as having run:\n{out}"
    );
}

/// The other half of the collapse: it started, ran, and came back non-zero.
/// This must NOT read like the case above -- telling them apart is the point.
///
/// Note what actually fails here: `sh` starts and cannot open the installer
/// path, because the stubbed `curl` wrote no file. That is a weaker scenario
/// than "the installer ran and failed", but it is the same `Exited` variant
/// through the same call site, which is what is being gated.
#[test]
fn a_child_that_runs_and_fails_says_that_instead() {
    let out = refresh_skills_with(&farm("exited", &["sh"]), "exited");
    assert!(
        out.contains("sh ran and exited"),
        "a child that ran and failed was not reported as having run:\n{out}"
    );
    assert!(
        !out.contains("could not be started"),
        "a child that ran was reported as never having started:\n{out}"
    );
}
