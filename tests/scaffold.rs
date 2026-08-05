//! Integration tests for scaffolding delegation.
//!
//! Phase 2 of the self-describing-client epic removed the Rust `generate` stubs:
//! `generate` (and migrate/seed/test/routes/metrics/queue/console) are no longer
//! client-owned — they are forwarded verbatim to the detected framework CLI,
//! which owns arg parsing and the real generators. These tests prove the
//! delegation wiring end-to-end with the built binary (no mocks).
//!
//! The tests that need a real framework toolchain (`generate crud`, `init`) are
//! `#[ignore]`d by default — run them with `cargo test -- --ignored`. The
//! project-detection tests run everywhere.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_tina4");
static COUNTER: AtomicU32 = AtomicU32::new(0);

fn unique_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("tina4-scaffold-{}-{}-{}", std::process::id(), tag, n));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run tina4")
}

/// Is a real `tina4python` on PATH? (Spawns it — no assumptions.)
fn tina4python_available() -> bool {
    Command::new("tina4python").arg("commands").output().is_ok()
}

// ── pass-through dispatch (no framework needed) ─────────────────────

#[test]
fn generate_outside_project_fails() {
    // `generate` now forwards to the framework CLI via the pass-through path.
    // With no project to detect, that path must error cleanly, not scaffold.
    let dir = unique_dir("no-project");
    let out = run(&dir, &["generate", "model", "Product"]);
    assert!(!out.status.success(), "generate outside a project must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("No Tina4 project"),
        "expected a 'No Tina4 project' error, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_command_is_forwarded_verbatim() {
    // A command the client doesn't own (with a --flag) must reach the
    // external-subcommand pass-through — not a clap "unexpected argument"
    // error. In a non-project dir the delegate then reports no project.
    let dir = unique_dir("frob");
    let out = run(&dir, &["frobnicate", "--wat", "42"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No Tina4 project"),
        "unknown command should hit pass-through -> delegate (no project), got: {stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ── generate delegates to the framework (real generator output) ─────

#[test]
#[ignore = "needs a locally-installed tina4python; run with: cargo test -- --ignored"]
fn generate_crud_delegates_to_framework() {
    // `crud` was NEVER a Rust generator — the deleted stub only handled
    // model/route/migration/middleware and printed "Unknown generator: crud".
    // So a WORKING `generate crud` that emits framework-shaped files proves the
    // client now delegates to the framework CLI.
    if !tina4python_available() {
        eprintln!("SKIP generate_crud_delegates_to_framework: tina4python not on PATH");
        return;
    }
    let dir = unique_dir("crud");
    // app.py alone => detected as python; no pyproject/uv.lock => resolve_cli
    // uses the globally-installed tina4python (no venv, no network).
    fs::write(dir.join("app.py"), "from tina4_python import Tina4\n").unwrap();

    let out = run(&dir, &["generate", "crud", "Widget"]);
    assert!(
        out.status.success(),
        "generate crud failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let model = dir.join("src/orm/Widget.py");
    assert!(model.is_file(), "framework model src/orm/Widget.py not generated");
    let body = fs::read_to_string(&model).unwrap();
    // Framework-only markers, absent from the old Rust stub (which imported
    // `from tina4_python import ORM, ...` and had no table_name):
    assert!(body.contains("table_name = \"widget\""), "not framework output:\n{body}");
    assert!(body.contains("from tina4_python.orm import"), "not framework import:\n{body}");
    // The framework's crud also emits routes + a real test the stub never did.
    assert!(dir.join("src/routes/widgets.py").is_file(), "framework routes missing");
    assert!(dir.join("tests/test_widgets.py").is_file(), "framework test missing");

    let _ = fs::remove_dir_all(&dir);
}

// ── init (gated: needs the language toolchain + network) ────────────

#[test]
#[ignore = "needs uv + network; run with: cargo test -- --ignored"]
fn init_python_scaffolds_runnable_project() {
    let base = unique_dir("init");
    let out = Command::new(BIN)
        .args(["init", "python", "app"])
        .current_dir(&base)
        .env("TINA4_INIT_NO_SERVE", "1")
        .output()
        .expect("run tina4 init");
    assert!(out.status.success(), "init failed: {}", String::from_utf8_lossy(&out.stderr));
    let proj = base.join("app");
    assert!(proj.join("app.py").is_file());
    assert!(proj.join("pyproject.toml").is_file());
    assert!(proj.join(".env").is_file());
    let _ = fs::remove_dir_all(&base);
}

#[test]
#[ignore = "needs npm + network; run with: cargo test -- --ignored"]
fn init_js_scaffolds_a_project_that_tests_and_audits_clean() {
    // Regression gate for four defects the tina4js scaffold shipped:
    //
    //   1. package.json declared `"test": "vitest run"` with no vitest
    //      dependency, so `npm test` died with "vitest: command not found"
    //      on every freshly created project.
    //   2. No test file and no tests/ directory, so once vitest was installed
    //      `npm test` still exited 1 with "No test files found".
    //   3. vite was pinned to ^5.4.0, which resolves to a version depending on
    //      esbuild <=0.24.2 (GHSA-67mh-4wv8-2f99), so `npm audit` reported two
    //      advisories on a brand new project.
    //   4. vite.config.ts used __dirname, which Vite's native config loader
    //      does not define and which becomes the default in a future major.
    //
    // Asserts on generated content rather than on npm's output, so the test
    // stays fast and does not depend on the advisory database of the day.
    let base = unique_dir("init-js");
    let out = Command::new(BIN)
        .args(["init", "js", "app"])
        .current_dir(&base)
        .env("TINA4_INIT_NO_SERVE", "1")
        .output()
        .expect("run tina4 init js");
    assert!(out.status.success(), "init js failed: {}", String::from_utf8_lossy(&out.stderr));
    let proj = base.join("app");

    let package_json = fs::read_to_string(proj.join("package.json")).expect("package.json");
    assert!(
        package_json.contains("\"vitest\""),
        "package.json declares a vitest test script but does not depend on vitest:\n{package_json}"
    );
    assert!(
        package_json.contains("\"jsdom\""),
        "tina4js touches HTMLElement on import, so tests need a DOM environment:\n{package_json}"
    );
    assert!(
        !package_json.contains("\"vite\": \"^5"),
        "vite ^5 pulls esbuild <=0.24.2 (GHSA-67mh-4wv8-2f99):\n{package_json}"
    );

    // The example test must exist AND be discoverable by vitest's default glob.
    let example = proj.join("tests/signals.test.ts");
    assert!(example.is_file(), "no example test, so `npm test` exits 1 on a new project");
    let example_body = fs::read_to_string(&example).unwrap();
    assert!(
        example_body.contains("expect("),
        "the example test must actually assert something:\n{example_body}"
    );

    let vite_config = fs::read_to_string(proj.join("vite.config.ts")).expect("vite.config.ts");
    // Match the CALL, not the word: the template explains the change in a
    // comment that necessarily mentions __dirname.
    assert!(
        !vite_config.contains("resolve(__dirname"),
        "__dirname is undefined under Vite's native config loader:\n{vite_config}"
    );
    assert!(
        vite_config.contains("resolve(import.meta.dirname"),
        "the alias must resolve from import.meta.dirname:\n{vite_config}"
    );
    assert!(
        vite_config.contains("environment: 'jsdom'"),
        "without a DOM environment every test file fails to import tina4js:\n{vite_config}"
    );

    let _ = fs::remove_dir_all(&base);
}
