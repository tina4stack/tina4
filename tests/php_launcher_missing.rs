//! A php project whose `vendor/` exists but holds no `vendor/bin/tina4php`
//! must say so, through both entry points that launch it.
//!
//! On 3.8.107 the `vendor/` check passed, `resolve_cli` fell back to a bare
//! `tina4php`, and the spawn failed with "program not found" (Windows) or
//! "No such file or directory (os error 2)" — naming neither the missing file
//! nor `composer install` (tina4#127). A global `tina4php` on PATH is still
//! used when there is one.
//!
//! These drive the shipped binary: the gap was in what `serve` and the
//! delegated commands checked before spawning, not in `resolve_cli` itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tina4-php-launcher-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A php project with `vendor/` present and no launcher in it, plus an empty
/// directory to use as PATH so no global `tina4php` can stand in.
fn project_without_launcher(tag: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(tag);
    fs::write(dir.join("composer.json"), "{\"require\":{\"tina4stack/tina4php\":\"*\"}}\n").unwrap();
    fs::write(dir.join(".env"), "TINA4_SECRET=0123456789abcdef0123456789abcdef\n").unwrap();
    fs::create_dir_all(dir.join("vendor").join("composer")).unwrap();
    let path = dir.join("empty-path");
    fs::create_dir_all(&path).unwrap();
    (dir, path)
}

/// Run tina4, capped: a blown cap is a named failure, not a wedged suite.
fn run_tina4(dir: &Path, path: &Path, args: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("USERPROFILE", dir)
        .env("PATH", path)
        .env("TINA4_NO_BROWSER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run tina4");
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("tina4 {:?} did not exit within 30s", args);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_names_the_launcher(out: &str) {
    assert!(
        out.contains("tina4php is missing") && out.contains("composer install"),
        "expected the missing launcher and `composer install` to be named, got:\n{}",
        out
    );
}

#[test]
fn serve_names_the_missing_launcher() {
    let (dir, path) = project_without_launcher("serve");
    let out = run_tina4(&dir, &path, &["serve", "--no-browser", "--port", "7493"]);
    assert_names_the_launcher(&out);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn delegated_command_names_the_missing_launcher() {
    let (dir, path) = project_without_launcher("routes");
    let out = run_tina4(&dir, &path, &["routes"]);
    assert_names_the_launcher(&out);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_global_tina4php_on_path_is_still_used() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, path) = project_without_launcher("global");
    let global = path.join("tina4php");
    fs::write(&global, "#!/bin/sh\necho \"GLOBAL tina4php $*\"\n").unwrap();
    fs::set_permissions(&global, fs::Permissions::from_mode(0o755)).unwrap();
    let sh_path = format!("{}:/bin:/usr/bin", path.display());
    let out = run_tina4(&dir, Path::new(&sh_path), &["routes"]);
    assert!(out.contains("GLOBAL tina4php routes"), "global tina4php not used, got:\n{}", out);
    let _ = fs::remove_dir_all(&dir);
}
