//! `tina4 update` deletes files off PATH before it does anything else, and the
//! deletion has no undo. These go through the real entry point on purpose: the
//! unit tests cover the classification, and every one of them stays green while
//! `clean_v2_binaries` is reverted to deleting silently, because none of them
//! runs the command.
//!
//! The runs are hermetic. PATH contains only the decoy directory, so the CLI
//! cannot find `curl` either, and `tina4 update` stops at the version check
//! immediately after the cleanup step we are testing. Nothing on the machine's
//! own PATH is reachable, and the binary under test is a copy, so an update
//! that did proceed could not overwrite the build artifact.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_tina4");

struct Sandbox {
    root: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn sandbox(tag: &str) -> Sandbox {
    let root = std::env::temp_dir().join(format!(
        "tina4-v2clean-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(root.join("decoys")).expect("create the decoy directory");
    std::fs::create_dir_all(root.join("home")).expect("create the sandbox home");
    // Run a copy: `tina4 update` replaces its own executable, and the build
    // artifact is not ours to destroy.
    std::fs::copy(BIN, root.join("tina4")).expect("copy the binary under test");
    let mut perms = std::fs::metadata(root.join("tina4")).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(root.join("tina4"), perms).unwrap();
    Sandbox { root }
}

fn decoy(sb: &Sandbox, name: &str, prints: &str) -> PathBuf {
    let path = sb.root.join("decoys").join(name);
    let mut f = std::fs::File::create(&path).expect("write the decoy");
    writeln!(f, "#!/bin/sh\necho \"{prints}\"").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// `tina4 update` with a PATH that reaches the decoys and nothing else.
fn run_update(sb: &Sandbox) -> String {
    let out = Command::new(sb.root.join("tina4"))
        .arg("update")
        .env_clear()
        .env("PATH", sb.root.join("decoys"))
        .env("HOME", sb.root.join("home"))
        .env("TERM", "dumb")
        .current_dir(&sb.root)
        .stdin(Stdio::null())
        .output();

    // Another test thread writing its own copy of the binary can leave this
    // one exec'ing a file the kernel still sees as open for write (ETXTBSY).
    // That is the harness, not the CLI, so retry rather than fail.
    let out = match out {
        Ok(out) => out,
        Err(e) if e.raw_os_error() == Some(26) => {
            std::thread::sleep(std::time::Duration::from_millis(100));
            Command::new(sb.root.join("tina4"))
                .arg("update")
                .env_clear()
                .env("PATH", sb.root.join("decoys"))
                .env("HOME", sb.root.join("home"))
                .env("TERM", "dumb")
                .current_dir(&sb.root)
                .stdin(Stdio::null())
                .output()
                .expect("run tina4 update after ETXTBSY")
        }
        Err(e) => panic!("run tina4 update: {e}"),
    };
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Survival alone proves nothing here: the consent gate keeps every file alive
/// on a pipe, so a test that only checks `exists()` stays green with the old
/// substring classifier put back. What discriminates is whether the binary was
/// ever NOMINATED -- the run prints every candidate it found.
fn assert_not_nominated(path: &Path, text: &str, why: &str) {
    assert!(
        path.exists(),
        "{why}\n`tina4 update` removed {}\nOutput was:\n{text}",
        path.display()
    );
    assert!(
        !text.contains(&path.to_string_lossy().to_string()),
        "{why}\n{} was listed as an old v2 binary, so a yes would have deleted it.\nOutput was:\n{text}",
        path.display()
    );
    assert!(
        !text.contains("v2 CLI binaries"),
        "{why}\nNothing should have been found at all.\nOutput was:\n{text}"
    );
}

/// The defect, at the real entry point: a current CLI that also names its
/// runtime was deleted, because `8.2.1` contains `2.`.
#[test]
fn update_leaves_a_current_cli_that_names_its_runtime() {
    let sb = sandbox("runtime");
    let php = decoy(&sb, "tina4php", "Tina4 PHP CLI 3.13.136 (PHP 8.2.1)");
    let text = run_update(&sb);
    assert_not_nominated(
        &php,
        &text,
        "3.13.136 is current; the 2. came from the PHP version.",
    );
}

/// The runtime is named FIRST and its major is 2. Reading only the leading
/// version token called this current CLI a v2 and offered to delete it.
#[test]
fn update_leaves_a_current_cli_whose_runtime_is_named_first() {
    let sb = sandbox("runtimefirst");
    let rb = decoy(&sb, "tina4ruby", "Ruby 2.7.0 -- Tina4 Ruby CLI 3.13.136");
    let text = run_update(&sb);
    assert_not_nominated(
        &rb,
        &text,
        "3.13.136 is this CLI's own version; the 2.7.0 belongs to Ruby.",
    );
}

/// A CLI that prints no version at all — which is every shipped v3 CLI as of
/// 2026-09-22 — is not evidence of anything and must be left alone.
#[test]
fn update_leaves_a_cli_whose_version_cannot_be_read() {
    let sb = sandbox("noversion");
    let py = decoy(&sb, "tina4python", "Unknown command: --version");
    let text = run_update(&sb);
    assert_not_nominated(&py, &text, "No version was readable, so nothing is proven.");
}

/// Even a genuine v2 is not deleted when nobody can be asked. A prompt on a
/// pipe would hang an unattended update instead; this must terminate, keep the
/// file, and say how to remove it by hand.
#[test]
fn update_without_a_terminal_removes_nothing_and_says_so() {
    let sb = sandbox("notty");
    let rb = decoy(&sb, "tina4ruby", "tina4ruby 2.1.4");
    let text = run_update(&sb);
    assert!(
        rb.exists(),
        "stdin is not a terminal, so consent is absent, yet {} was deleted.\nOutput was:\n{text}",
        rb.display()
    );
    assert!(
        text.contains("Not a terminal"),
        "the user has to be told what was found and how to remove it:\n{text}"
    );
    assert!(
        text.contains(&rb.to_string_lossy().to_string()),
        "the path it declined to delete has to be named:\n{text}"
    );
}

/// The detection itself still works: a v2 is still found and reported, it is
/// only the deletion that now needs a yes.
#[test]
fn update_still_identifies_a_real_v2() {
    let sb = sandbox("identify");
    decoy(&sb, "tina4ruby", "tina4ruby 2.1.4");
    let text = run_update(&sb);
    assert!(
        text.contains("v2 CLI binaries"),
        "a genuine v2 must still be reported:\n{text}"
    );
}
