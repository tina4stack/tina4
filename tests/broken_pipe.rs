//! The CLI must compose with Unix pipelines without printing a Rust panic when
//! the downstream consumer closes stdout early (`tina4 metrics --json | head`).

#![cfg(unix)]

use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_tina4");

#[test]
fn a_closed_stdout_pipe_never_prints_a_rust_panic() {
    let mut child = Command::new(BIN)
        .args(["metrics", "--path", "src", "--json", "--top", "99999"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start tina4 metrics");

    drop(child.stdout.take().expect("capture stdout"));

    let output = child.wait_with_output().expect("wait for tina4 metrics");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("Broken pipe"),
        "closed stdout leaked an implementation panic: {stderr}"
    );
}
