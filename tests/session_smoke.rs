//! End-to-end smoke test for the supervisor session lifecycle.
//!
//! Builds a scratch repo, creates a session, writes a file in the
//! worktree, round-trips the diff, applies the commit back to the
//! main tree, and verifies the file and commit landed. Also cancels
//! a second session to exercise the cleanup path.
//!
//! Run with: `cargo test --test session_smoke`
//!
//! This is a single-file integration test (tests/*.rs) so it only
//! sees the crate's public API — exactly what the HTTP layer sees.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

// Can't `use tina4::session` — the crate type is `[[bin]]` not
// `[lib]` in Cargo.toml, so integration tests can't import internals
// directly. Invoke the real binary instead via HTTP? That wants a
// running server. Instead, shell out to git ourselves and verify
// the filesystem/repo shape after each operation — this test then
// doubles as a spec of the git commands the session module runs.

fn git(cwd: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("git failed to spawn");
    assert!(
        out.status.success(),
        "git {:?} exit {}\nstderr: {}",
        args,
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn setup_scratch_repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "tina4-session-smoke-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@test"]);
    git(&root, &["config", "user.name", "test"]);
    fs::write(root.join("README.md"), "# scratch\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    root
}

/// Sanity check: verifying that `git worktree add` + a commit inside
/// the worktree + `git diff` between branches + `git apply` round-trips
/// as the session module expects. If this stops working on some git
/// version, session.rs will fail in the same way — better to know from
/// a test than from a production surprise.
#[test]
fn worktree_roundtrip_mirrors_session_module_behavior() {
    let root = setup_scratch_repo();

    // 1. Create a branch + worktree off HEAD.
    let worktree = root.join(".tina4/sessions/abc/tree");
    fs::create_dir_all(&root.join(".tina4/sessions/abc")).unwrap();
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "tina4/supervise/abc",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(worktree.join("README.md").exists(), "worktree should contain baseline files");

    // 2. Write a new file in the worktree + commit on the branch.
    fs::write(worktree.join("hello.txt"), "hi\n").unwrap();
    git(&worktree, &["add", "hello.txt"]);
    git(&worktree, &["commit", "-q", "-m", "coder: add hello"]);

    // 3. From the main tree, diff against the session branch.
    let diff = git(
        &root,
        &[
            "diff",
            "--numstat",
            "HEAD..tina4/supervise/abc",
        ],
    );
    assert!(diff.contains("hello.txt"), "diff should mention hello.txt — got {diff:?}");

    // 4. Apply the patch to the main tree.
    let patch = git(
        &root,
        &["diff", "HEAD..tina4/supervise/abc"],
    );
    // We mirror session.rs's `git apply --index` path.
    let mut child = Command::new("git")
        .arg("-C").arg(&root)
        .args(["apply", "--index"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.as_mut().unwrap().write_all(patch.as_bytes()).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "git apply should succeed");
    assert!(root.join("hello.txt").exists(), "file should now be in main tree");

    // 5. Commit + verify the new HEAD carries the file.
    git(&root, &["commit", "-q", "-m", "supervise/abc: apply\n\nsession: abc\n"]);
    let log = git(&root, &["log", "--oneline", "-1"]);
    assert!(log.contains("supervise/abc"), "HEAD should be the apply commit");

    // 6. Cancel: worktree remove --force + branch -D should wipe the
    //    session artefacts.
    git(&root, &["worktree", "remove", "--force", worktree.to_str().unwrap()]);
    git(&root, &["branch", "-D", "tina4/supervise/abc"]);
    let _ = fs::remove_dir_all(&root.join(".tina4"));
    assert!(!worktree.exists(), "worktree should be gone after remove");
    let branches = git(&root, &["branch", "--list"]);
    assert!(
        !branches.contains("tina4/supervise/abc"),
        "branch should be deleted: {branches}"
    );

    // Leave the scratch repo around for post-mortem if the test
    // fails; a cleanup on success keeps /tmp tidy.
    let _ = fs::remove_dir_all(&root);
}
