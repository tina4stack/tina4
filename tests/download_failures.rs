//! `tina4 docs` and `tina4 books` must say what actually went wrong, and fail.
//!
//! These go through the real entry points on purpose. The unit tests cover the
//! classification and the wording, and every one of them stays green while the
//! call sites are reverted to the original `if !download_file(...)` — the whole
//! user-visible defect can come back under a green suite unless something runs
//! the command itself.
//!
//! The failure is forced without privileges by creating the destination as a
//! **directory**: curl cannot write its output there, whoever it is running as.
//! That leaves the cause dependent on the machine — offline it fails at connect
//! instead — so the assertions below hold for any cause, and the disk-specific
//! one is checked only when the run actually produced a write failure.

#![cfg(unix)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_tina4");

/// Run one subcommand in a directory where its download cannot be written.
fn run_with_unwritable_destination(sub: &str, zip_name: &str, project: bool) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "tina4-dlfail-{}-{}-{}",
        sub,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(dir.join(zip_name)).expect("create the blocking directory");
    if project {
        // `tina4 docs` needs a project to detect; app.py is enough.
        std::fs::write(dir.join("app.py"), b"").expect("write app.py");
    }

    let out = Command::new(BIN)
        .arg(sub)
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("run tina4 {sub}: {e}"));

    std::fs::remove_dir_all(&dir).ok();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

fn assert_reports_the_real_failure(sub: &str, zip_name: &str, project: bool) {
    let (code, text) = run_with_unwritable_destination(sub, zip_name, project);

    assert_eq!(
        code, 1,
        "`tina4 {sub}` exited {code} after a failed download — a script cannot \
         tell that apart from success. Output was:\n{text}"
    );
    assert!(
        text.contains("Could not download"),
        "`tina4 {sub}` did not name the failure. Output was:\n{text}"
    );
    assert!(
        !text.contains("Download failed. Check your connection"),
        "`tina4 {sub}` still blames the connection for whatever went wrong. \
         Output was:\n{text}"
    );

    // Only when this machine really did fail to write: then, and only then, the
    // advice has to be about the disk.
    if text.contains("could not write the file") || text.contains("SIGXFSZ") {
        assert!(
            text.contains("free space"),
            "`tina4 {sub}` diagnosed a write failure and gave no disk advice. \
             Output was:\n{text}"
        );
    }
}

#[test]
fn books_reports_the_real_reason_and_exits_nonzero() {
    assert_reports_the_real_failure("books", "tina4-book.zip", false);
}

#[test]
fn docs_reports_the_real_reason_and_exits_nonzero() {
    assert_reports_the_real_failure("docs", ".tina4-docs.zip", true);
}
