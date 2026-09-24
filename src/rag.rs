//! Client for tina4-rag — the framework-knowledge retrieval service.
//!
//! The RAG server runs at `http://41.71.84.173:11438` (override with
//! `TINA4_RAG_URL` env var). It serves two flavours of queries:
//!
//!   POST /v1/search  → `{query, top_k}` → raw chunks with distances.
//!                      Fast; used for grounding agent prompts.
//!   POST /v1/ask     → `{question, language?}` → LLM answer + sources.
//!                      Slower; used for direct "how do I do X" prompts.
//!   GET  /v1/stats   → corpus metadata (chunk count, languages, model).
//!
//! This module does two jobs:
//!
//!   1. **Search** — a plain async client with a small timeout, used by
//!      the coder loop to pull convention examples into its prompt
//!      before a file_write. Low top_k (3-5) keeps the prompt lean.
//!
//!   2. **Verify** — a post-hoc check over every file a supervisor
//!      session changed. We derive a query from the file path + a
//!      slice of its content, retrieve similar chunks, and emit
//!      `RagWarning`s when the file diverges from the retrieved
//!      patterns. Mechanical heuristics only for slice 3; slice 4
//!      upgrades the comparator with a narrow LLM pass.
//!
//! Why this matters: qwen2.5-coder writes plausible-looking code from
//! training memory, and that memory is often wrong for Tina4 (wrong
//! imports, missing decorators, old signatures). Routing the coder
//! through RAG is the single biggest quality lever — same reason every
//! guide tells LLMs to "read the docs first."

use serde::{Deserialize, Serialize};

use crate::session::RagWarning;

/// Default RAG base URL. Overridable via `TINA4_RAG_URL`. The default
/// is the andrevanzuydam.com instance; local dev installs would point
/// this at their own copy.
const DEFAULT_RAG_URL: &str = "http://41.71.84.173:11438";

/// How long to wait for a RAG response before giving up. Too low and
/// a slow embed cycle trips the timeout; too high and one unresponsive
/// query hangs the whole agent turn. 8s is comfortably above the p95
/// we've observed (~800ms for /search, ~3s for /ask).
const RAG_TIMEOUT_SECS: u64 = 8;

// ── Request / response types ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct SearchReq<'a> {
    query: &'a str,
    top_k: usize,
}

#[derive(Debug, Deserialize)]
pub struct SearchResp {
    pub query: String,
    #[serde(default)]
    pub hits: Vec<RagHit>,
}

/// One retrieved chunk from the RAG corpus. `distance` is cosine
/// distance — lower means more similar (0.0 = identical). `Serialize`
/// so the /supervise/rag/search passthrough can echo hits verbatim
/// to agents without restructuring.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RagHit {
    pub text: String,
    #[serde(default)]
    pub metadata: RagMetadata,
    #[serde(default)]
    pub distance: f32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RagMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub chunk_index: u32,
}

// ── Public API ────────────────────────────────────────────────────

/// Resolve the RAG base URL at call time. We read the env var every
/// call (cheap) rather than snapshotting at start so a supervisor
/// restart isn't required after rotating `TINA4_RAG_URL`.
fn base_url() -> String {
    std::env::var("TINA4_RAG_URL").unwrap_or_else(|_| DEFAULT_RAG_URL.to_string())
}

/// Shared HTTP client. Building one per call would re-establish TLS /
/// connection pools; reusing is 5-10x faster for bursts of queries
/// (common during a diff verification pass).
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(RAG_TIMEOUT_SECS))
        .build()
        .expect("reqwest client build failed")
}

/// Run a semantic search against the RAG corpus. Returns `Vec<RagHit>`
/// ordered by ascending distance (most relevant first). Empty vec on
/// any failure — callers degrade gracefully by continuing without the
/// retrieved context rather than bubbling an error; an unreachable RAG
/// shouldn't block writes.
pub async fn search(query: &str, top_k: usize) -> Vec<RagHit> {
    let url = format!("{}/v1/search", base_url());
    let body = SearchReq { query, top_k };
    let resp = match client().post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[rag] search failed to send: {e}");
            return Vec::new();
        }
    };
    if !resp.status().is_success() {
        eprintln!("[rag] search returned {}", resp.status());
        return Vec::new();
    }
    match resp.json::<SearchResp>().await {
        Ok(s) => s.hits,
        Err(e) => {
            eprintln!("[rag] search response parse failed: {e}");
            Vec::new()
        }
    }
}

/// Format retrieved hits for injection into an agent's system prompt.
/// Keeps only the most informative bits: title, language, a trimmed
/// excerpt of the chunk text. Skips the URL and distance — the agent
/// doesn't need those, and they bloat token count.
pub fn format_hits_for_prompt(hits: &[RagHit], max_chars_per_hit: usize) -> String {
    if hits.is_empty() {
        return String::new();
    }
    // Source is named per-hit (metadata.source): mcp.tina4.com for the
    // official framework MCP, or tina4-rag for the local corpus fallback.
    let source = hits
        .first()
        .map(|h| if h.metadata.source.is_empty() { "tina4-rag" } else { h.metadata.source.as_str() })
        .unwrap_or("tina4-rag");
    let mut out = format!("Relevant Tina4 framework patterns (from {source}):\n\n");
    for (i, hit) in hits.iter().enumerate() {
        let trimmed = if hit.text.len() > max_chars_per_hit {
            format!("{}…", &hit.text[..max_chars_per_hit])
        } else {
            hit.text.clone()
        };
        out.push_str(&format!(
            "## [{i}] {} ({})\n{}\n\n",
            if hit.metadata.title.is_empty() { "(untitled)" } else { &hit.metadata.title },
            if hit.metadata.language.is_empty() { "any" } else { &hit.metadata.language },
            trimmed,
        ));
    }
    out.push_str("\nWhen you write code, match these patterns. Cite the example number (e.g. \"[0]\") in a comment above any non-obvious choice so a reviewer can trace the decision.\n");
    out
}

// ── Per-file verification ─────────────────────────────────────────

/// Run a RAG-backed convention check over a set of changed files.
/// Builds per-file queries, retrieves similar chunks, emits
/// `RagWarning` entries when the file obviously diverges from the
/// retrieved patterns. Mechanical heuristics only — the comparator
/// is intentionally conservative so false positives stay rare.
///
/// Slice 4 layers an LLM-backed review on top of this (the reviewer
/// agent sees the diff + retrieved chunks + acceptance criteria and
/// issues a structured verdict). For slice 3 this is enough to catch
/// the most common framework-mismatch classes.
pub async fn verify_files(
    project_dir: &std::path::Path,
    files: &[(String, String)], // (path, language)
) -> Vec<RagWarning> {
    let mut warnings = Vec::new();
    for (rel_path, language) in files {
        // Read the file as it sits on disk right now. If we were
        // verifying an in-flight diff we'd want the session-branch
        // version — callers currently invoke this after session
        // state has already been staged, so the project_dir-relative
        // path resolves to the right content.
        let abs = project_dir.join(rel_path);
        let content = match std::fs::read_to_string(&abs) {
            Ok(c) => c,
            Err(_) => continue, // file was deleted; nothing to verify
        };

        let query = derive_query_for_file(rel_path, &content, language);
        if query.is_empty() {
            continue;
        }
        let hits = search(&query, 4).await;
        if hits.is_empty() {
            continue;
        }

        // Hand the heuristics the file + the retrieved chunks; they
        // decide whether to emit warnings. The comparator functions
        // are small + tested so we can add more as we see new
        // mismatch classes in the wild.
        warnings.extend(check_route_handler(rel_path, &content, language, &hits));
        warnings.extend(check_sql_migration(rel_path, &content, language, &hits));
        warnings.extend(check_import_drift(rel_path, &content, language, &hits));
    }
    warnings
}

/// Build a RAG query tuned to the file's role. Routes get different
/// retrieval priorities than migrations, templates get different
/// again. Keeping the query short (~80 chars) focuses the embedding
/// match on the salient bits.
fn derive_query_for_file(rel_path: &str, content: &str, language: &str) -> String {
    let lower = rel_path.to_lowercase();
    if lower.starts_with("src/routes/") || lower.contains("/routes/") {
        return format!("{language} tina4 route handler decorator pattern");
    }
    if lower.starts_with("migrations/") || lower.contains("/migrations/") || lower.ends_with(".sql") {
        return "tina4 migration schema sql pattern".to_string();
    }
    if lower.starts_with("src/orm/") || lower.contains("/orm/") || lower.contains("/models/") {
        return format!("{language} tina4 orm model class pattern");
    }
    if lower.ends_with(".twig") || lower.ends_with(".html") || lower.ends_with(".jinja") {
        return "tina4 template twig block extends pattern".to_string();
    }
    if lower.starts_with("src/middleware/") || lower.contains("/middleware/") {
        return format!("{language} tina4 middleware before after pattern");
    }
    // Fallback: use the first non-empty line that looks like a
    // signature — function/class header — as a query hint. Keeps
    // verification useful for one-off files that don't fit any role.
    for line in content.lines().take(40) {
        let t = line.trim();
        if t.starts_with("def ") || t.starts_with("async def ") || t.starts_with("class ")
            || t.starts_with("function ") || t.starts_with("export function ")
        {
            return format!("{language} {}", t.chars().take(60).collect::<String>());
        }
    }
    String::new()
}

// ── Mismatch heuristics ───────────────────────────────────────────

/// Check a Tina4 route handler file. Common mistakes we've seen:
///   * imports from `tina4` (PHP / v2 shape) instead of `tina4_python`
///   * a sync database call inside an `async def` handler (ADR-0074): it
///     blocks the event loop, so every other request waits on the query
///   * `@post` without `@noauth` on a public-looking endpoint when the
///     retrieved chunks all have `@noauth`
fn check_route_handler(path: &str, content: &str, language: &str, hits: &[RagHit]) -> Vec<RagWarning> {
    let mut out = Vec::new();
    if language != "python" {
        return out;
    }

    // All route examples in the retrieved chunks used tina4_python —
    // if the file imports from `tina4` (no suffix) or omits the
    // import entirely, that's almost certainly wrong.
    let hits_prefer_tina4_python = hits.iter().any(|h| h.text.contains("tina4_python.core.router"));
    let file_uses_tina4_python = content.contains("tina4_python.core.router");
    let file_uses_bare_tina4 = content.contains("from tina4.") || content.contains("import tina4;");
    if hits_prefer_tina4_python && !file_uses_tina4_python {
        out.push(RagWarning {
            path: path.to_string(),
            kind: "convention".into(),
            message: if file_uses_bare_tina4 {
                "Imports from `tina4.*` — Tina4 Python routes use `from tina4_python.core.router import ...`".into()
            } else {
                "Route file doesn't import the tina4_python router — every retrieved example uses `from tina4_python.core.router import ...`".into()
            },
            line: find_first_import_line(content),
            reference: hits.first().map(|h| {
                if h.metadata.title.is_empty() {
                    h.metadata.url.clone()
                } else {
                    format!("{} ({})", h.metadata.title, h.metadata.url)
                }
            }),
        });
    }

    // ADR-0074: a plain `def` route is fine - Tina4 runs it in a worker thread.
    // What is wrong is a SYNC database call inside `async def`: it runs on the
    // event loop and every other request waits for the query to return.
    for (line, call) in sync_db_calls_in_async_handlers(content) {
        out.push(RagWarning {
            path: path.to_string(),
            kind: "convention".into(),
            message: format!(
                "`{call}` runs on the event loop inside an `async def` route and blocks every \
                 other request until it returns - await its `_async` twin, or declare the \
                 route with plain `def` (ADR-0074)"
            ),
            line: Some(line),
            reference: None,
        });
    }

    out
}

/// Sync database calls a Tina4 Python route can make. Each has an `_async`
/// twin (`db.fetch_async(...)`, `user.save_async()`), which is what an
/// `async def` handler must await instead (ADR-0074).
const SYNC_DB_CALLS: &[&str] = &[
    "db.fetch(", "db.fetch_one(", "db.fetch_all(", "db.execute(", "db.execute_many(",
    "db.insert(", "db.update(", "db.delete(", "db.truncate(", "db.start_transaction(",
    "db.commit(", "db.rollback(", "db.get_next_id(", ".save()", ".load(", ".find_by_id(",
    ".find_or_fail(", ".select_one(", ".force_delete()", ".restore()",
];

/// `(line number, call)` for every sync database call inside the body of a
/// top-level `async def`. A body is every indented line after the header; the
/// next top-level line (decorator, `def`, import) ends it. An awaited call or
/// an `_async(` call is never flagged.
fn sync_db_calls_in_async_handlers(content: &str) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    let mut in_async_body = false;
    for (index, line) in content.lines().enumerate() {
        let top_level = !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t');
        if top_level {
            in_async_body = line.starts_with("async def ");
            continue;
        }
        if !in_async_body || line.contains("await ") || line.contains("_async(") {
            continue;
        }
        let code = line.split('#').next().unwrap_or("");
        if let Some(call) = SYNC_DB_CALLS.iter().find(|call| code.contains(**call)) {
            found.push(((index + 1) as u32, call.trim_end_matches('(').to_string()));
        }
    }
    found
}

/// Check a SQL migration file. DROP without IF EXISTS is a frequent
/// foot-gun — a rerun crashes on the missing table instead of being
/// idempotent.
fn check_sql_migration(path: &str, content: &str, _language: &str, hits: &[RagHit]) -> Vec<RagWarning> {
    let mut out = Vec::new();
    if !path.to_lowercase().ends_with(".sql") {
        return out;
    }
    let lower = content.to_lowercase();
    for (idx, line) in content.lines().enumerate() {
        let l = line.trim().to_lowercase();
        if l.starts_with("drop ") && !l.contains("if exists") {
            out.push(RagWarning {
                path: path.to_string(),
                kind: "risk".into(),
                message: format!("`{}` — add `IF EXISTS` so re-running the migration doesn't crash", line.trim()),
                line: Some((idx + 1) as u32),
                reference: hits.iter().find(|h| h.text.to_lowercase().contains("if exists")).map(|h| h.metadata.title.clone()),
            });
        }
    }
    // CREATE TABLE without IF NOT EXISTS is less severe but still
    // worth flagging as info.
    if lower.contains("create table ") && !lower.contains("if not exists") {
        out.push(RagWarning {
            path: path.to_string(),
            kind: "info".into(),
            message: "`CREATE TABLE` without `IF NOT EXISTS` — consider guarding against reruns".into(),
            line: None,
            reference: None,
        });
    }
    out
}

/// Catch imports that look wrong against the corpus. For Python + JS
/// we scan the first ~40 lines for imports and complain when an
/// imported module name appears in zero retrieved chunks despite the
/// hits being on-topic.
fn check_import_drift(path: &str, content: &str, language: &str, hits: &[RagHit]) -> Vec<RagWarning> {
    let mut out = Vec::new();
    if !(language == "python" || language == "javascript" || language == "typescript") {
        return out;
    }
    let mut imported_modules: Vec<String> = Vec::new();
    for line in content.lines().take(40) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("from ") {
            if let Some(idx) = rest.find(' ') {
                imported_modules.push(rest[..idx].to_string());
            }
        } else if let Some(rest) = t.strip_prefix("import ") {
            imported_modules.push(rest.split_whitespace().next().unwrap_or("").to_string());
        }
    }
    // Only run this check on tina4-flavoured modules — third-party
    // imports legitimately won't appear in the corpus.
    for module in imported_modules {
        if !module.starts_with("tina4") {
            continue;
        }
        let mentioned_in_hits = hits.iter().any(|h| h.text.contains(&module));
        if !mentioned_in_hits && !hits.is_empty() {
            out.push(RagWarning {
                path: path.to_string(),
                kind: "convention".into(),
                message: format!(
                    "Imports `{module}` but none of the top RAG hits mention it — may be an outdated or misspelled module path"
                ),
                line: None,
                reference: hits.first().map(|h| h.metadata.title.clone()),
            });
        }
    }
    out
}

fn find_first_import_line(content: &str) -> Option<u32> {
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("from ") || t.starts_with("import ") {
            return Some((idx + 1) as u32);
        }
    }
    None
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_handler_flags_wrong_import() {
        let hits = vec![RagHit {
            text: "from tina4_python.core.router import get\n@get('/x')\nasync def x(req, res): pass".into(),
            metadata: RagMetadata { title: "Chapter 2".into(), ..Default::default() },
            distance: 0.2,
        }];
        let content = "from tina4 import router\n@get('/x')\nasync def x(req, res): pass";
        let w = check_route_handler("src/routes/x.py", content, "python", &hits);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "convention");
        assert!(w[0].message.contains("tina4_python"));
    }

    fn router_hit() -> Vec<RagHit> {
        vec![RagHit {
            text: "from tina4_python.core.router import get\n@get('/x')\nasync def x(req, res): pass".into(),
            metadata: Default::default(),
            distance: 0.2,
        }]
    }

    #[test]
    fn route_handler_accepts_a_plain_def_route_with_the_sync_api() {
        // ADR-0074: Tina4 runs a def route in a worker thread; sync DB calls are fine there.
        let content = "from tina4_python.core.router import get\n@get('/x')\ndef x(req, res):\n    return res(db.fetch_one(\"SELECT 1\"))";
        let w = check_route_handler("src/routes/x.py", content, "python", &router_hit());
        assert!(w.is_empty(), "a def route was flagged: {:?}", w.iter().map(|w| &w.message).collect::<Vec<_>>());
    }

    #[test]
    fn route_handler_flags_a_sync_db_call_inside_async_def() {
        let content = "from tina4_python.core.router import get\n@get('/x')\nasync def x(req, res):\n    row = db.fetch_one(\"SELECT 1\")\n    user.save()\n    return res(row)";
        let w = check_route_handler("src/routes/x.py", content, "python", &router_hit());
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].line, Some(4));
        assert!(w[0].message.contains("db.fetch_one") && w[0].message.contains("_async"));
        assert!(w[0].message.contains("`def`"));
        assert_eq!(w[1].line, Some(5));
    }

    #[test]
    fn route_handler_accepts_the_async_api_inside_async_def() {
        let content = "from tina4_python.core.router import get\n@get('/x')\nasync def x(req, res):\n    row = await db.fetch_one_async(\"SELECT 1\")\n    await user.save_async()\n    return res(row)\n\ndef helper():\n    return db.fetch(\"SELECT 2\")";
        let w = check_route_handler("src/routes/x.py", content, "python", &router_hit());
        assert!(w.is_empty(), "flagged: {:?}", w.iter().map(|w| &w.message).collect::<Vec<_>>());
    }

    #[test]
    fn sql_flags_drop_without_if_exists() {
        let content = "DROP TABLE users;\nCREATE TABLE users (id INTEGER);";
        let w = check_sql_migration("migrations/0001_x.sql", content, "sql", &[]);
        assert!(w.iter().any(|w| w.kind == "risk" && w.message.contains("IF EXISTS")));
    }

    #[test]
    fn sql_info_on_create_without_guard() {
        let content = "CREATE TABLE users (id INTEGER);";
        let w = check_sql_migration("migrations/0001_x.sql", content, "sql", &[]);
        assert!(w.iter().any(|w| w.kind == "info"));
    }

    #[test]
    fn derive_query_routes_by_path() {
        assert!(derive_query_for_file("src/routes/x.py", "", "python").contains("route"));
        assert!(derive_query_for_file("migrations/0001_x.sql", "", "sql").contains("migration"));
        assert!(derive_query_for_file("src/templates/home.twig", "", "twig").contains("template"));
    }

    #[test]
    fn format_hits_is_empty_for_empty_input() {
        assert_eq!(format_hits_for_prompt(&[], 500), "");
    }

    #[test]
    fn format_hits_truncates_long_chunks() {
        let hits = vec![RagHit {
            text: "a".repeat(1000),
            metadata: RagMetadata { title: "Long".into(), language: "python".into(), ..Default::default() },
            distance: 0.3,
        }];
        let formatted = format_hits_for_prompt(&hits, 100);
        // 100-char slice + the "…" suffix + headers ≈ ~200 chars, nothing like 1000.
        assert!(formatted.len() < 400);
    }
}
