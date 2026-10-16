//! A `.env` that starts with a UTF-8 byte-order mark must read like one that
//! does not, through every entry point that reads it.
//!
//! Windows PowerShell 5.1's `-Encoding utf8` writes the mark, as does any editor
//! saving "UTF-8 with BOM". On 3.8.88 it stayed glued to the first key, because
//! `str::trim` does not strip U+FEFF:
//!
//!   * `tina4 serve` exported `\u{feff}FIRST_KEY`, so the framework never saw
//!     `FIRST_KEY` at all — silently, every other variable intact;
//!   * `tina4 env --sync` wrote the glued key back below the others, moving the
//!     mark into the middle of the file where no parser strips it;
//!   * `tina4 env --migrate` could not see a legacy first key to rename it.
//!
//! These drive the shipped binary: the defect was in what the entry points did
//! with the file, not in any one helper.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const BOM: &str = "\u{feff}";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tina4-dotenv-bom-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Every key in a `.env`, read WITHOUT the CLI's parser and without stripping
/// anything a real consumer would not strip — a mark mid-file stays in its key.
fn keys_in(path: &Path) -> Vec<String> {
    let contents = fs::read_to_string(path).expect("read .env");
    let contents = contents.strip_prefix(BOM).unwrap_or(&contents);
    contents
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_string()))
        .collect()
}

fn run_tina4(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tina4"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .output()
        .expect("run tina4")
}

#[test]
fn env_sync_keeps_the_first_key_and_does_not_move_the_mark() {
    let dir = temp_dir("sync");
    let env_path = dir.join(".env");
    fs::write(&env_path, format!("{}FIRST_KEY=first\nSECOND_KEY=second\n", BOM)).unwrap();

    let out = run_tina4(&dir, &["env", "--sync"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let written = fs::read_to_string(&env_path).unwrap();
    let body = written.strip_prefix(BOM).unwrap_or(&written);
    assert!(
        !body.contains(BOM),
        "--sync moved the byte-order mark into the file:\n{:?}",
        written
    );
    let keys = keys_in(&env_path);
    assert!(keys.iter().any(|k| k == "FIRST_KEY"), "FIRST_KEY lost: {:?}", keys);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn env_migrate_renames_a_legacy_first_key_and_backs_up_byte_for_byte() {
    let dir = temp_dir("migrate");
    let env_path = dir.join(".env");
    let original = format!("{}DATABASE_URL=sqlite:///x.db\nSECRET=s\n", BOM);
    fs::write(&env_path, &original).unwrap();

    let out = run_tina4(&dir, &["env", "--migrate", "--yes"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let keys = keys_in(&env_path);
    assert!(
        keys.iter().any(|k| k == "TINA4_DATABASE_URL"),
        "the first key was not migrated: {:?}",
        keys
    );
    assert!(keys.iter().any(|k| k == "TINA4_SECRET"), "{:?}", keys);
    assert_eq!(
        fs::read_to_string(dir.join(".env.bak")).unwrap(),
        original,
        "the backup must be the file as it was, mark included"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// The language server inherits the CLI's environment; a stand-in dumps it.
#[cfg(unix)]
#[test]
fn serve_exports_the_first_key_of_a_bom_env() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("serve");
    let dump = dir.join("dump.txt");
    let bin_dir = dir.join("stub-bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // `tina4 serve` on a Python project runs `python3 app.py --managed`.
    fs::write(dir.join("requirements.txt"), "tina4_python\n").unwrap();
    fs::write(dir.join("app.py"), "").unwrap();
    fs::write(
        dir.join(".env"),
        format!("{}FIRST_KEY=first\nSECOND_KEY=second\nTINA4_PORT=8923\n", BOM),
    )
    .unwrap();
    let stub = bin_dir.join("python3");
    fs::write(&stub, format!("#!/bin/sh\nenv > '{}'\nexit 0\n", dump.display())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .arg("serve")
        .current_dir(&dir)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .env("HOME", &dir)
        .env("TINA4_NO_BROWSER", "true")
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
    assert!(dump.exists(), "the stand-in server never started within 30s");

    let dumped = fs::read_to_string(&dump).unwrap();
    assert!(
        dumped.lines().any(|l| l == "FIRST_KEY=first"),
        "FIRST_KEY never reached the server:\n{}",
        dumped
    );
    assert!(
        !dumped.contains(BOM),
        "a key reached the server with the mark glued on:\n{}",
        dumped
    );

    let _ = fs::remove_dir_all(&dir);
}
