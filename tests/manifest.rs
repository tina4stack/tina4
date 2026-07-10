//! End-to-end test for the Phase-2 manifest fingerprint cache, driven through
//! the built `tina4` binary against a REAL manifest-capable framework CLI (no
//! mocks). Proves: first `--help` spawns the CLI and writes `.tina4/commands.json`;
//! a second `--help` with an unchanged fingerprint REUSES the cache (no re-query);
//! `--refresh` re-queries and rewrites it.
//!
//! Gated: needs a framework CLI new enough to answer `commands --json` (the
//! Phase-1 release). The released global `tina4python` on most machines predates
//! it, so this is `#[ignore]`d — point `TINA4_MANIFEST_CLI` at a manifest-capable
//! CLI and run:
//!
//!   TINA4_MANIFEST_CLI=/abs/path/to/tina4python \
//!     cargo test --test manifest -- --ignored

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_tina4");

fn manifest_cli() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("TINA4_MANIFEST_CLI")?);
    path.exists().then_some(path)
}

/// A python project whose RESOLVED framework CLI is `cli`: `app.py` makes it
/// detect as python, and a `.venv/bin/tina4python` symlink to `cli` (with no
/// pyproject/uv.lock) makes `resolve_cli` pick it directly — no uv, no network.
fn project_using(cli: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tina4-manifest-e2e-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let venv_bin = dir.join(".venv").join("bin");
    fs::create_dir_all(&venv_bin).unwrap();
    fs::write(dir.join("app.py"), "from tina4_python import Tina4\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(cli, venv_bin.join("tina4python")).unwrap();
    #[cfg(windows)]
    fs::copy(cli, dir.join(".venv").join("Scripts").join("tina4python.exe"))
        .expect("windows: copy cli into .venv/Scripts");
    dir
}

fn help(dir: &Path, extra: &[&str]) -> String {
    let mut args = vec!["--help"];
    args.extend_from_slice(extra);
    let out = Command::new(BIN).args(&args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
#[ignore = "needs a manifest-capable tina4python; set TINA4_MANIFEST_CLI and run with --ignored"]
fn manifest_cache_populate_reuse_refresh() {
    let cli = match manifest_cli() {
        Some(cli) => cli,
        None => {
            eprintln!("SKIP manifest_cache_populate_reuse_refresh: set TINA4_MANIFEST_CLI");
            return;
        }
    };
    let dir = project_using(&cli);
    let cache = dir.join(".tina4").join("commands.json");
    assert!(!cache.exists(), "precondition: no cache yet");

    // 1) First --help spawns the CLI, writes the cache, lists discovered commands.
    let first = help(&dir, &[]);
    assert!(first.contains("Discovered from python"), "help missing discovered block:\n{first}");
    assert!(first.contains("migrate:create"), "help missing a discovered command:\n{first}");
    assert!(cache.exists(), "cache file was not written");
    let cache_body = fs::read_to_string(&cache).unwrap();
    assert!(cache_body.contains("\"fingerprint\""), "cache missing fingerprint:\n{cache_body}");
    assert!(cache_body.contains("migrate:create"));

    // 2) Rewrite the cache with a SENTINEL command name but the SAME fingerprint.
    //    A second --help WITHOUT --refresh must REUSE it (unchanged fingerprint =>
    //    fast path, no spawn), so the sentinel appears in the listing.
    let sentinel_body = cache_body.replace("migrate:create", "sentinelcmd");
    fs::write(&cache, &sentinel_body).unwrap();
    let reused = help(&dir, &[]);
    assert!(reused.contains("sentinelcmd"), "unchanged fingerprint must reuse the cache:\n{reused}");
    assert!(!reused.contains("migrate:create"), "must not have re-queried the CLI:\n{reused}");
    assert!(
        fs::read_to_string(&cache).unwrap().contains("sentinelcmd"),
        "reuse must not rewrite the cache"
    );

    // 3) --refresh must re-query the real CLI and overwrite the sentinel.
    let refreshed = help(&dir, &["--refresh"]);
    assert!(refreshed.contains("migrate:create"), "--refresh must re-query:\n{refreshed}");
    assert!(!refreshed.contains("sentinelcmd"), "--refresh must overwrite the sentinel:\n{refreshed}");
    let after = fs::read_to_string(&cache).unwrap();
    assert!(after.contains("migrate:create") && !after.contains("sentinelcmd"));

    let _ = fs::remove_dir_all(&dir);
}
