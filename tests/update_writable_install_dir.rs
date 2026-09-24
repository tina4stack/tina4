// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `tina4 update` must find out it cannot write to its own install directory
//! *before* it downloads, and say what to do about it.
//!
//! This goes through the real entry point on purpose. A unit test on the probe
//! function stays green while the call site is reverted, and the whole
//! user-visible defect comes back under it: a 9MB transfer that ends in
//! `curl: (23)` and advice to check free disk space, on a machine whose disk is
//! fine. The `Trying <asset>` assertion below is what pins the check ahead of
//! the transfer rather than merely present somewhere.
//!
//! Needs the network. The release check runs first and has to: a user already
//! on the latest version can still refresh skills, and must not be turned away
//! for a directory the update was never going to touch. When that check cannot
//! complete, or when this build is already current, the run proves nothing and
//! the test says so instead of asserting on a message it did not provoke.
//!
//! Read that last sentence as a limit, not a reassurance: on a checkout whose
//! crate version equals the latest release -- which is the normal state of
//! `main`, and therefore of CI -- this test SKIPS and still reports success. It
//! earns its place for anyone building from a tag behind the release, and it is
//! not the gate. The gate is `prove.sh` in
//! `testing-tina4/scratch/update-cannot-write-to-its-own-install-directory`,
//! which builds a stock and a fixed binary with the version lowered so the
//! download path is always reached, and which fails against unfixed source.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_tina4");

struct ReadOnlyInstall {
    dir: std::path::PathBuf,
}

impl Drop for ReadOnlyInstall {
    fn drop(&mut self) {
        // Restore write permission first or the directory cannot be removed,
        // and a leftover read-only tree breaks the next run of this suite.
        std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o755)).ok();
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Copy the built binary into a directory, make that directory unwritable, and
/// run `update` from it. The binary has to live there: the code under test asks
/// `current_exe` where it is, so pointing it at a read-only directory from the
/// outside would test nothing.
fn run_update_from_unwritable_dir() -> (ReadOnlyInstall, i32, String) {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    );
    let dir = std::env::temp_dir().join(format!("tina4-update-ro-{}", unique));
    std::fs::create_dir_all(&dir).expect("create install dir");
    let exe = dir.join("tina4");
    std::fs::copy(BIN, &exe).expect("copy binary");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod binary");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod dir");

    let guard = ReadOnlyInstall { dir };
    // Hermetic: `tina4 update` first sweeps PATH for old CLIs to delete and then
    // refreshes skills under HOME, so neither may be the developer's own.
    let home = std::env::temp_dir().join(format!("tina4-update-ro-home-{}", unique));
    std::fs::create_dir_all(&home).expect("create home");
    let out = Command::new(&exe)
        .arg("update")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .output()
        .expect("run update");
    std::fs::remove_dir_all(&home).ok();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (guard, out.status.code().unwrap_or(-1), text)
}

#[test]
fn update_refuses_before_downloading_when_the_install_dir_is_not_writable() {
    let (install, code, text) = run_update_from_unwritable_dir();

    if text.contains("Could not check latest version") {
        eprintln!("skipped: no network, the release check never completed");
        return;
    }
    if text.contains("already up to date") {
        eprintln!("skipped: this build is the latest release, so nothing downloads");
        return;
    }
    if text.contains("not downgrading") {
        // On a checkout AHEAD of the published latest (the normal state of
        // `main`), the self-update downgrade guard exits before the download
        // path, exactly like "already up to date". Same class of skip; the gate
        // is prove.sh, which lowers the version so the download path is reached.
        eprintln!("skipped: this build is newer than the latest release, so nothing downloads");
        return;
    }

    let dir = install.dir.display().to_string();
    assert!(
        text.contains(&dir),
        "the message must name the directory that is not writable; got:\n{text}"
    );
    assert!(
        text.contains("sudo tina4 update"),
        "the message must say what to run instead; got:\n{text}"
    );
    // The point of the fix: no transfer is started at all.
    assert!(
        !text.contains("Trying "),
        "update downloaded before checking it could write; got:\n{text}"
    );
    assert!(
        !install.dir.join("tina4.tmp").exists(),
        "a partial download was left in the install directory"
    );
    assert_ne!(code, 0, "a refused update must be visible to its caller");
}
