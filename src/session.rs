//! Supervisor session lifecycle — git worktree + branch per session.
//!
//! One active piece of work = one git worktree at
//! `<project>/.tina4/sessions/<id>/tree/` on branch `tina4/supervise/<id>`.
//! Sub-agents do their writes in the worktree through the existing MCP
//! tools (file_write / file_patch / migration_create) — the isolation
//! comes from the CWD switch, not from replacing the tools. Each
//! logical unit of work becomes a commit on the session branch with a
//! structured trailer (agent / step / plan / files) so the commit log
//! *is* the audit trail.
//!
//! Hand-back to dev-admin streams a proposal event; user clicks Apply,
//! which copies the accepted files from the session worktree into the
//! user's working tree and commits them there with a squash message.
//! Reject = remove worktree + delete branch.
//!
//! Why not libgit2? `std::process::Command` git calls are portable,
//! introspectable (a user can `git log tina4/supervise/abc` to see
//! what happened), and we don't need git2 for anything else. Keeping
//! the dep surface at zero for this feature matches tina4's philosophy.
//!
//! Why git at all? Staging-area designs reinvent branches, merge,
//! partial apply, history, and crash recovery — all of which git
//! already has. See the design discussion in the dev-admin commit
//! history for details.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metadata persisted alongside each session worktree. Small — the
/// authoritative history is git itself. This file is just enough to
/// reconnect a session after a restart without re-reading every ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub branch: String,
    /// Absolute path to the worktree root.
    pub worktree: PathBuf,
    /// Initial prompt / title used when the session was created.
    pub title: String,
    /// Plan slug this session is tied to (empty if off-plan).
    #[serde(default)]
    pub plan: String,
    /// Unix millis.
    pub created_at: u128,
    /// Commit SHA on the user's branch when the session forked.
    /// Used on commit so we can warn if the target has diverged.
    pub base_sha: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffFile {
    pub path: String,
    /// "A" (added) / "M" (modified) / "D" (deleted) / "R" (renamed).
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDiff {
    pub id: String,
    pub branch: String,
    pub base_sha: String,
    pub files: Vec<DiffFile>,
    pub commits: Vec<SessionCommit>,
    /// Convention + quality warnings populated by `rag_check`. Empty
    /// in slice 2; the next slice wires real tina4-rag queries
    /// against the changed files to catch off-framework code before
    /// the user hits Apply. Shape is intentional even at zero items
    /// so the dev-admin panel can render the warnings section
    /// unconditionally.
    #[serde(default)]
    pub warnings: Vec<RagWarning>,
}

/// A single convention/quality concern attached to a changed file.
/// Surfaced in the Diff tab next to the offending file so the user
/// sees framework-mismatch warnings before they Apply the proposal.
#[derive(Debug, Clone, Serialize)]
pub struct RagWarning {
    pub path: String,
    /// "convention" — doesn't match framework idiom (wrong decorator,
    ///   missing helper, off-pattern route shape).
    /// "risk"       — potentially dangerous (DROP without IF EXISTS,
    ///   SQL injection shape, secret-ish value).
    /// "info"       — worth knowing but not blocking (untested code
    ///   path, unusual import).
    pub kind: String,
    pub message: String,
    /// Optional anchor line in the changed file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Citation from tina4-rag: where the correct convention lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionCommit {
    pub sha: String,
    pub subject: String,
    /// Parsed trailer values (agent, step, plan, files). Empty map if
    /// the commit didn't use the structured format.
    pub trailer: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitResult {
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    /// The new commit SHA on the user's branch (squash-commit).
    pub sha: String,
    /// Warnings the user should see (diverged base, partial apply, etc.)
    pub warnings: Vec<String>,
}

/// Error type — strings, not an enum, because every failure here is
/// surfaced to the UI as-is and variants would just create translation
/// ceremony. If we grow richer handling later, this becomes an enum.
pub type SessionError = String;
pub type Result<T> = std::result::Result<T, SessionError>;

// ─── Public API ───────────────────────────────────────────────────

/// Create a new session: branch off HEAD, add a worktree under
/// `.tina4/sessions/<id>/tree/`, persist metadata, return the info
/// the caller needs to hand back to dev-admin.
///
/// If the project isn't a git repo yet, `ensure_git_repo` will try to
/// `git init` + make an empty initial commit so `git worktree add` has
/// a HEAD to branch from. Only fails if git itself isn't on PATH.
pub fn create_session(project_dir: &Path, title: &str, plan: &str) -> Result<SessionMeta> {
    ensure_git_repo(project_dir)?;

    let id = new_session_id();
    let branch = format!("tina4/supervise/{id}");
    let sessions_root = project_dir.join(".tina4").join("sessions");
    let session_dir = sessions_root.join(&id);
    let worktree = session_dir.join("tree");

    fs::create_dir_all(&session_dir).map_err(|e| format!("mkdir sessions dir: {e}"))?;

    // Resolve the base SHA before branching so we can record where
    // this session forked — `git rev-parse HEAD` at worktree-add time
    // is the exact same commit the new worktree checks out.
    let base_sha = git_stdout(project_dir, &["rev-parse", "HEAD"])
        .map_err(|e| format!("rev-parse HEAD: {e}"))?
        .trim()
        .to_string();

    // `git worktree add -b <branch> <path> HEAD` creates the branch
    // and checks it out in the new worktree in a single call. Cleaner
    // than branch-then-worktree because there's no intermediate state
    // we'd have to roll back on failure.
    git_run(
        project_dir,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().ok_or("worktree path is non-utf8")?,
            "HEAD",
        ],
    )
    .map_err(|e| format!("git worktree add: {e}"))?;

    let meta = SessionMeta {
        id: id.clone(),
        branch: branch.clone(),
        worktree: worktree.clone(),
        title: title.to_string(),
        plan: plan.to_string(),
        created_at: unix_ms(),
        base_sha,
    };

    write_meta(&session_dir, &meta)?;
    Ok(meta)
}

/// List every active session (anything under `.tina4/sessions/` with
/// a valid meta.json and an existing worktree). Dead entries — meta
/// file without worktree or vice versa — are skipped silently; they
/// get cleaned up by `cancel_session`.
pub fn list_sessions(project_dir: &Path) -> Vec<SessionMeta> {
    let sessions_root = project_dir.join(".tina4").join("sessions");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&sessions_root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(meta) = read_meta(&path) else { continue };
        if !meta.worktree.exists() {
            continue;
        }
        out.push(meta);
    }
    // Newest first — matches the dev-admin expectation that the most
    // recent session is the "current" one.
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

/// Produce a diff summary between the session branch and its fork
/// point. Files + per-file additions/deletions + commit log with
/// parsed trailers. Dev-admin renders this in the Diff tab.
pub fn diff_session(project_dir: &Path, id: &str) -> Result<SessionDiff> {
    let meta = load_session(project_dir, id)?;

    // Compare session branch against its merge-base with the base SHA.
    // Using `merge-base` keeps the diff accurate even if the user
    // committed on their own branch after the session forked — we
    // show only what the session itself introduced.
    let merge_base = git_stdout(project_dir, &["merge-base", &meta.base_sha, &meta.branch])
        .map_err(|e| format!("merge-base: {e}"))?
        .trim()
        .to_string();

    let numstat = git_stdout(
        project_dir,
        &[
            "diff",
            "--numstat",
            &format!("{}..{}", merge_base, meta.branch),
        ],
    )
    .map_err(|e| format!("git diff --numstat: {e}"))?;

    let name_status = git_stdout(
        project_dir,
        &[
            "diff",
            "--name-status",
            &format!("{}..{}", merge_base, meta.branch),
        ],
    )
    .map_err(|e| format!("git diff --name-status: {e}"))?;

    // Zip numstat and name-status by path. numstat gives counts but
    // uses "-" for binary files; name-status gives A/M/D/R. Joining
    // them per-path produces one `DiffFile` with both views.
    let mut statuses = std::collections::HashMap::new();
    for line in name_status.lines() {
        let mut parts = line.splitn(2, '\t');
        let st = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();
        if !path.is_empty() {
            statuses.insert(path, st);
        }
    }

    let mut files = Vec::new();
    for line in numstat.lines() {
        let mut cols = line.split('\t');
        let add = cols.next().unwrap_or("0");
        let del = cols.next().unwrap_or("0");
        let path = cols.next().unwrap_or("").to_string();
        if path.is_empty() {
            continue;
        }
        let status = statuses.remove(&path).unwrap_or_else(|| "M".to_string());
        files.push(DiffFile {
            path,
            status,
            additions: add.parse().unwrap_or(0),
            deletions: del.parse().unwrap_or(0),
        });
    }

    // Commit log on the session branch — one line per commit, with
    // trailers parsed out of the body. The trailer is what makes
    // `git log` a usable audit format for the supervisor.
    let log_format = "%H%x1f%s%x1f%b%x1e";
    let log = git_stdout(
        project_dir,
        &[
            "log",
            &format!("--format={log_format}"),
            &format!("{}..{}", merge_base, meta.branch),
        ],
    )
    .map_err(|e| format!("git log: {e}"))?;

    let mut commits = Vec::new();
    for record in log.split('\x1e') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }
        let mut fields = record.split('\x1f');
        let sha = fields.next().unwrap_or("").to_string();
        let subject = fields.next().unwrap_or("").to_string();
        let body = fields.next().unwrap_or("");
        if sha.is_empty() {
            continue;
        }
        commits.push(SessionCommit {
            sha,
            subject,
            trailer: parse_trailer(body),
        });
    }

    // Slice 3 will populate this from tina4-rag queries over each
    // changed file. For slice 2 we return an empty vec so the shape
    // is stable — dev-admin renders a "no concerns" state until
    // warnings flow in.
    let warnings = rag_check_stub(&files);

    Ok(SessionDiff {
        id: meta.id,
        branch: meta.branch,
        base_sha: meta.base_sha,
        files,
        commits,
        warnings,
    })
}

/// Placeholder — slice 3 replaces this with real tina4-rag calls.
/// Kept as a separate function so the wire-up surface is ready and
/// callers don't need to change when the real implementation lands.
fn rag_check_stub(_files: &[DiffFile]) -> Vec<RagWarning> {
    Vec::new()
}

/// Apply the session's changes to the user's working tree.
/// `accept` is a list of paths to apply; empty means "apply all."
/// Implementation: generate the diff for the accepted paths, apply
/// it to the user's tree with `git apply`, then commit. We don't
/// use `git merge --squash` because that requires the branch to be
/// in the same worktree, and we want surgical per-file control.
///
/// After a successful apply, the session worktree + branch are
/// *not* removed — the user can revise and apply again. Call
/// `cancel_session` to clean up once the work is done.
pub fn commit_session(project_dir: &Path, id: &str, accept: &[String]) -> Result<CommitResult> {
    let meta = load_session(project_dir, id)?;
    let mut warnings = Vec::new();

    // Warn if the user's branch moved since the session forked. The
    // apply can still succeed in most cases; we just tell them so they
    // can eyeball the result.
    let current_head = git_stdout(project_dir, &["rev-parse", "HEAD"])
        .map_err(|e| format!("rev-parse HEAD: {e}"))?
        .trim()
        .to_string();
    if current_head != meta.base_sha {
        warnings.push(format!(
            "base moved: session forked from {}, current HEAD is {}",
            short_sha(&meta.base_sha),
            short_sha(&current_head)
        ));
    }

    let merge_base = git_stdout(project_dir, &["merge-base", &meta.base_sha, &meta.branch])
        .map_err(|e| format!("merge-base: {e}"))?
        .trim()
        .to_string();

    // Build the diff args. If accept is empty = apply everything; else
    // pass `-- <paths>` to git diff to restrict the hunks.
    let range = format!("{}..{}", merge_base, meta.branch);
    let mut diff_args: Vec<String> = vec!["diff".into(), range];
    if !accept.is_empty() {
        diff_args.push("--".into());
        for p in accept {
            diff_args.push(p.clone());
        }
    }

    let diff_text = git_stdout(
        project_dir,
        &diff_args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    )
    .map_err(|e| format!("git diff: {e}"))?;

    if diff_text.trim().is_empty() {
        return Err("nothing to apply — session has no changes for the given paths".into());
    }

    // `git apply --index` both applies to the working tree and stages
    // the result, which is exactly what we want for the subsequent
    // commit. --3way lets apply resolve small conflicts using the blob
    // metadata, useful if the user's branch has drifted.
    let apply_result = git_run_stdin(
        project_dir,
        &["apply", "--index", "--3way"],
        diff_text.as_bytes(),
    );

    // Which paths actually got applied? Simplest way: parse the diff
    // headers (`diff --git a/<p> b/<p>`) and intersect with what's now
    // staged. If apply reported success assume everything landed.
    let requested = diff_paths(&diff_text);
    let (applied, skipped) = match apply_result {
        Ok(()) => (requested.clone(), Vec::new()),
        Err(e) => {
            return Err(format!(
                "git apply failed: {e}\n\n\
                Likely cause: the target files drifted after the session forked. \
                Revise the session or cancel and restart."
            ));
        }
    };

    // Build a human-readable commit message from the session commits.
    let diff_summary = diff_session(project_dir, id).unwrap_or(SessionDiff {
        id: id.to_string(),
        branch: meta.branch.clone(),
        base_sha: meta.base_sha.clone(),
        files: Vec::new(),
        commits: Vec::new(),
        warnings: Vec::new(),
    });

    let subject = if meta.title.is_empty() {
        format!("supervise/{id}")
    } else {
        meta.title.clone()
    };

    let mut body = String::from("\n\n");
    for c in &diff_summary.commits {
        body.push_str(&format!("- {} ({})\n", c.subject, short_sha(&c.sha)));
    }
    if !diff_summary.commits.is_empty() {
        body.push('\n');
    }
    body.push_str(&format!("session: {id}\n"));
    body.push_str(&format!("branch: {}\n", meta.branch));
    if !meta.plan.is_empty() {
        body.push_str(&format!("plan: {}\n", meta.plan));
    }
    if !applied.is_empty() {
        body.push_str(&format!("files: {}\n", applied.join(", ")));
    }

    let full_msg = format!("{subject}{body}");
    git_run(project_dir, &["commit", "-m", &full_msg])
        .map_err(|e| format!("git commit: {e}"))?;

    let sha = git_stdout(project_dir, &["rev-parse", "HEAD"])
        .map_err(|e| format!("rev-parse after commit: {e}"))?
        .trim()
        .to_string();

    Ok(CommitResult {
        applied,
        skipped,
        sha,
        warnings,
    })
}

/// Remove the session worktree and its branch. Idempotent — if the
/// worktree is already gone we just drop the metadata file. Never
/// touches the user's own branches or commits.
pub fn cancel_session(project_dir: &Path, id: &str) -> Result<()> {
    let meta = load_session(project_dir, id)?;

    // `worktree remove --force` tolerates locally-modified files in the
    // worktree, which can happen if an agent wrote something but didn't
    // commit. Cancelling is a "throw it all away" operation, so force
    // is the right default.
    let _ = git_run(
        project_dir,
        &[
            "worktree",
            "remove",
            "--force",
            meta.worktree
                .to_str()
                .ok_or("worktree path is non-utf8")?,
        ],
    );

    // Branch delete — `-D` because the branch's work is being discarded
    // and may not be merged anywhere. If the branch doesn't exist
    // (worktree remove already dropped it), we swallow the error.
    let _ = git_run(project_dir, &["branch", "-D", &meta.branch]);

    // Remove the session's metadata dir (`.tina4/sessions/<id>/`). The
    // worktree subdir is usually already gone at this point; any
    // remaining files (meta.json) go too.
    let session_dir = project_dir
        .join(".tina4")
        .join("sessions")
        .join(&meta.id);
    let _ = fs::remove_dir_all(&session_dir);

    Ok(())
}

// ─── Structured commit trailer ────────────────────────────────────

/// Compose the body-only portion of a session-branch commit message.
/// Matches the format `parse_trailer` expects. Keep these in sync.
///
/// Example output:
/// ```text
/// coder: step 2 — add POST /contact
///
/// files: src/routes/contact.py
/// plan: implement-contact-form
/// step: 2
/// confidence: 0.85
/// evidence: src/routes/ping.py:12
/// ```
///
/// First line is human-readable for `git log --oneline`. The blank
/// line + key:value block is a machine-readable trailer that
/// `parse_trailer` extracts on read.
pub fn compose_commit_message(
    subject: &str,
    files: &[String],
    plan: Option<&str>,
    step: Option<u32>,
    agent: Option<&str>,
    confidence: Option<f32>,
    evidence: &[String],
) -> String {
    let mut out = subject.to_string();
    out.push_str("\n\n");
    if !files.is_empty() {
        out.push_str(&format!("files: {}\n", files.join(", ")));
    }
    if let Some(p) = plan {
        out.push_str(&format!("plan: {p}\n"));
    }
    if let Some(s) = step {
        out.push_str(&format!("step: {s}\n"));
    }
    if let Some(a) = agent {
        out.push_str(&format!("agent: {a}\n"));
    }
    if let Some(c) = confidence {
        out.push_str(&format!("confidence: {:.2}\n", c));
    }
    if !evidence.is_empty() {
        out.push_str(&format!("evidence: {}\n", evidence.join(", ")));
    }
    out
}

fn parse_trailer(body: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Accept `key: value` where key is a short lower-case token.
        // Skips prose lines that happen to contain colons ("URL: ...")
        // by requiring the key to be <24 chars and ASCII-alphanumeric.
        if let Some(colon) = line.find(':') {
            let key = &line[..colon];
            if key.len() <= 24
                && !key.is_empty()
                && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            {
                let value = line[colon + 1..].trim().to_string();
                out.insert(key.to_string(), value);
            }
        }
    }
    out
}

// ─── Helpers ──────────────────────────────────────────────────────

fn ensure_git_repo(project_dir: &Path) -> Result<()> {
    // Fast path: the project is already a git repo.
    if git_run(project_dir, &["rev-parse", "--git-dir"]).is_ok() {
        return Ok(());
    }

    // No git dir. Two failure modes:
    //   1. git binary missing on PATH   → unfixable here; report clearly.
    //   2. git binary present, no repo  → auto-init. The supervisor needs
    //      a repo so it can `git worktree add` a throwaway branch; asking
    //      the user to run `git init` manually is friction for something
    //      the CLI can do in one shell-out. We also drop a permissive
    //      initial commit so `worktree add HEAD` has a HEAD to branch
    //      from (a brand-new repo has no commits and the next step would
    //      fail with "fatal: invalid reference: HEAD").
    if which::which("git").is_err() {
        return Err(format!(
            "{} is not a git repository and `git` was not found on PATH. \
             Install git (https://git-scm.com/downloads) and retry.",
            project_dir.display()
        ));
    }

    git_run(project_dir, &["init"])
        .map_err(|e| format!("git init failed in {}: {e}", project_dir.display()))?;

    // A freshly-initialised repo has no HEAD. `git worktree add ... HEAD`
    // in create_session() needs one, so we make an empty initial commit.
    // --allow-empty keeps us from having to stage a placeholder file.
    git_run(project_dir, &["commit", "--allow-empty", "-m", "tina4: initial commit"])
        .map_err(|e| format!("git commit (initial) failed in {}: {e}", project_dir.display()))?;

    Ok(())
}

fn load_session(project_dir: &Path, id: &str) -> Result<SessionMeta> {
    // Defend against path traversal via id. Session ids are hex from
    // new_session_id(); anything with a slash or .. is suspect.
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!("invalid session id: {id}"));
    }
    let session_dir = project_dir.join(".tina4").join("sessions").join(id);
    if !session_dir.exists() {
        return Err(format!("session {id} not found"));
    }
    read_meta(&session_dir)
}

fn read_meta(session_dir: &Path) -> Result<SessionMeta> {
    let path = session_dir.join("meta.json");
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse meta.json: {e}"))
}

fn write_meta(session_dir: &Path, meta: &SessionMeta) -> Result<()> {
    let path = session_dir.join("meta.json");
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("serialize meta: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

fn new_session_id() -> String {
    // 8 hex chars of millisecond timestamp + 4 hex chars of entropy
    // from SystemTime's nanos. No RNG dep needed — collision risk over
    // a human-scale session count is nil.
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ms = d.as_millis() as u64;
    let ns = (d.subsec_nanos() & 0xffff) as u16;
    format!("{:08x}{:04x}", ms & 0xffff_ffff, ns)
}

fn unix_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Extract the a-side paths from `diff --git a/X b/X` headers. Good
/// enough for our apply reporting — we don't need to reconstruct
/// rename details, just "which files got touched?"
fn diff_paths(diff: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some(space) = rest.find(' ') {
                let p = &rest[..space];
                if !out.iter().any(|x: &String| x == p) {
                    out.push(p.to_string());
                }
            }
        }
    }
    out
}

// ─── Thin wrappers around `git` ──────────────────────────────────

fn git_run(cwd: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} exited {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn git_run_stdin(cwd: &Path, args: &[&str], stdin: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn git: {e}"))?;
    if let Some(mut s) = child.stdin.take() {
        s.write_all(stdin).map_err(|e| format!("write stdin: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("wait git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} exited {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(format!(
            "git {} exited {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_parses_typical_block() {
        let body = "\nfiles: src/x.py, src/y.py\nplan: contact-form\nstep: 2\nagent: coder\nconfidence: 0.85\n";
        let t = parse_trailer(body);
        assert_eq!(t.get("plan").map(String::as_str), Some("contact-form"));
        assert_eq!(t.get("step").map(String::as_str), Some("2"));
        assert_eq!(t.get("agent").map(String::as_str), Some("coder"));
    }

    #[test]
    fn trailer_ignores_prose_colons() {
        let body = "\nSee the docs at https://example.com\nplan: foo\n";
        let t = parse_trailer(body);
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("plan").map(String::as_str), Some("foo"));
    }

    #[test]
    fn compose_and_parse_roundtrip() {
        let msg = compose_commit_message(
            "coder: step 2 — add POST /contact",
            &["src/routes/contact.py".into()],
            Some("contact-form"),
            Some(2),
            Some("coder"),
            Some(0.85),
            &["src/routes/ping.py:12".into()],
        );
        // First line is the subject.
        assert_eq!(msg.lines().next().unwrap(), "coder: step 2 — add POST /contact");
        // Trailer reconstructs cleanly.
        let body = msg.splitn(2, "\n\n").nth(1).unwrap_or("");
        let t = parse_trailer(body);
        assert_eq!(t.get("plan").map(String::as_str), Some("contact-form"));
        assert_eq!(t.get("step").map(String::as_str), Some("2"));
        assert_eq!(t.get("agent").map(String::as_str), Some("coder"));
        assert_eq!(t.get("confidence").map(String::as_str), Some("0.85"));
    }

    #[test]
    fn session_id_is_hex_and_stable_length() {
        let id = new_session_id();
        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// A directory that is not yet a git repo should get one auto-created
    /// by `ensure_git_repo`, complete with an initial commit so the next
    /// step (`git worktree add HEAD`) has a HEAD to branch from. This
    /// lets users start a supervisor session in a fresh project without
    /// having to run `git init` themselves.
    #[test]
    fn ensure_git_repo_auto_inits_empty_dir() {
        let dir = std::env::temp_dir().join(format!(
            "tina4-autoinit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Configure git locally so the auto-commit doesn't error on CI
        // machines with no global git identity.
        let _ = Command::new("git")
            .arg("-C").arg(&dir)
            .args(["init", "-q"])
            .output();
        let _ = Command::new("git")
            .arg("-C").arg(&dir)
            .args(["config", "user.email", "t@t"])
            .output();
        let _ = Command::new("git")
            .arg("-C").arg(&dir)
            .args(["config", "user.name", "t"])
            .output();
        // Wipe .git to simulate a non-repo project directory, but keep
        // the global-config shim via env var so auto-init's commit call
        // doesn't explode on a clean CI box.
        let _ = fs::remove_dir_all(dir.join(".git"));
        std::env::set_var("GIT_AUTHOR_NAME", "t");
        std::env::set_var("GIT_AUTHOR_EMAIL", "t@t");
        std::env::set_var("GIT_COMMITTER_NAME", "t");
        std::env::set_var("GIT_COMMITTER_EMAIL", "t@t");

        assert!(!dir.join(".git").exists(), "precondition: no .git yet");
        ensure_git_repo(&dir).expect("auto-init should succeed when git is on PATH");
        assert!(dir.join(".git").exists(), ".git should exist after auto-init");

        // A second call is a no-op — the fast path should hit `rev-parse`
        // and return Ok without trying to re-init or re-commit.
        ensure_git_repo(&dir).expect("second call should be a no-op");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_paths_extracts_both_sides() {
        let diff = "\
diff --git a/src/x.py b/src/x.py
index 1234..5678 100644
--- a/src/x.py
+++ b/src/x.py
@@ -1 +1 @@
-old
+new
diff --git a/README.md b/README.md
new file mode 100644
";
        let paths = diff_paths(diff);
        assert_eq!(paths, vec!["src/x.py", "README.md"]);
    }
}
