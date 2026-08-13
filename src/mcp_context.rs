//! Client for **mcp.tina4.com** — the official Tina4 framework-grounding MCP.
//!
//! This is the *framework* MCP (version-current API + real examples from the
//! live corpus), distinct from the project's *local* dev MCP (`/__dev/mcp`,
//! which serves project actions: files, routes, DB, plans). The agent coder
//! composes both: local MCP to act on the project, this one to write correct,
//! idiomatic Tina4 code.
//!
//! Transport: Streamable-HTTP JSON-RPC at `${TINA4_MCP_URL}/mcp` (default
//! `https://mcp.tina4.com`). Auth is a Bearer token — free at
//! https://profile.tina4.com — supplied via `TINA4_MCP_TOKEN`. We resolve the
//! token from the process env first, then fall back to the project `.env`, so a
//! token pasted into the dev-admin lands on disk and takes effect on the next
//! turn WITHOUT restarting the agent.
//!
//! Grounding is best-effort: any failure (no token, unreachable host, bad
//! response) returns an empty `Vec<RagHit>` and the caller degrades to the
//! local `tina4-rag` corpus — an unconfigured or offline framework MCP must
//! never block a write.

use std::path::Path;

use serde_json::json;

use crate::rag::{RagHit, RagMetadata};

/// Default base URL for the official framework MCP. Override with
/// `TINA4_MCP_URL` (e.g. to point at a self-hosted mirror). The `/mcp`
/// JSON-RPC path is appended by `endpoint()`.
const DEFAULT_MCP_URL: &str = "https://mcp.tina4.com";

/// Timeout for a grounding call. Retrieval is corpus-side and usually sub-second,
/// but the first call after a cold start can take a couple of seconds; 12s keeps
/// a slow round-trip from hanging the coder turn while staying well above p95.
const MCP_TIMEOUT_SECS: u64 = 12;

/// Timeout for a `long_context` reasoning call. Reasoning over a large context
/// can take tens of seconds to a few minutes, so this is generous — far above
/// the grounding timeout. A slow reasoning turn is still preferable to falling
/// back to the smaller thinking model, but we cap it so a wedged call can't hang
/// the agent forever (the caller degrades to the thinking model on timeout).
const LONG_CONTEXT_TIMEOUT_SECS: u64 = 300;

/// The env var that holds the Bearer token.
pub const TOKEN_VAR: &str = "TINA4_MCP_TOKEN";

// ── Config resolution ─────────────────────────────────────────────

/// Base URL, read every call (cheap) so rotating `TINA4_MCP_URL` needs no
/// restart — same policy as `rag::base_url`. Public so `agent::load_chat_settings`
/// can stamp it onto the `thinking` slot's `ModelSettings.url`.
pub fn base_url() -> String {
    std::env::var("TINA4_MCP_URL").unwrap_or_else(|_| DEFAULT_MCP_URL.to_string())
}

/// Full JSON-RPC endpoint (`<base>/mcp`), trimming any trailing slash on the
/// configured base so `https://mcp.tina4.com/` and `.../` both resolve cleanly.
fn endpoint() -> String {
    format!("{}/mcp", base_url().trim_end_matches('/'))
}

/// The shared **FREE-TOKEN** trial credential. Sent by default when the
/// developer has set no personal `TINA4_MCP_TOKEN`, so the dev-admin coder and
/// grounding work *before* signup — the whole point of the trial. Andre
/// activates the literal `FREE-TOKEN` as a **rate-limited** credential on
/// tina4.com's auth (mcp.tina4.com, and chat.tina4.com/general for the hosted
/// reasoning model). Overridable at deploy via `TINA4_FREE_TOKEN` to rotate it
/// without a rebuild. Set to `""` to disable the free rung entirely.
const FREE_TOKEN: &str = "FREE-TOKEN";

/// The env var a developer sets to use their OWN mcp.tina4.com Bearer token.
/// (Re-exported name kept as `TOKEN_VAR` above for back-compat.)
pub const FREE_TOKEN_VAR: &str = "TINA4_FREE_TOKEN";

/// Which credential the supervisor resolved for mcp.tina4.com grounding — drives
/// the dev-admin status line and the "register for your own" signup nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    /// The developer's own token (process env or project `.env`).
    Personal,
    /// The shared FREE-TOKEN trial credential — nudge them to register.
    Free,
    /// No credential at all (free rung disabled AND no personal token).
    None,
}

/// The FREE-TOKEN value: `TINA4_FREE_TOKEN` env override, else the compiled
/// constant. Split from the env read so the resolution is unit-testable without
/// mutating the global process environment (see `free_token_from`).
fn free_token_from(env_override: Option<&str>) -> Option<String> {
    let v = env_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| FREE_TOKEN.to_string());
    let v = v.trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

/// The FREE-TOKEN value in effect (env override → compiled constant → None).
pub fn free_token() -> Option<String> {
    free_token_from(std::env::var(FREE_TOKEN_VAR).ok().as_deref())
}

/// The developer's OWN token: process env first, then the project `.env`
/// fallback. Blank values are treated as absent so an empty `TINA4_MCP_TOKEN=`
/// line doesn't mask a real token set in the environment. `None` means the
/// developer has not configured their own — which is what triggers the free
/// trial fallback and the signup nudge.
pub fn personal_token(project_dir: &Path) -> Option<String> {
    if let Ok(t) = std::env::var(TOKEN_VAR) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    read_env_file_value(project_dir, TOKEN_VAR)
}

/// Pure resolution of (token, source) from the personal + free candidates.
/// No env, no filesystem — the whole resolution order lives here so it can be
/// exhaustively unit-tested (a pure function over its inputs, not a mock).
fn resolve(personal: Option<String>, free: Option<String>) -> (Option<String>, TokenSource) {
    if let Some(p) = personal.filter(|s| !s.trim().is_empty()) {
        return (Some(p), TokenSource::Personal);
    }
    match free.filter(|s| !s.trim().is_empty()) {
        Some(f) => (Some(f), TokenSource::Free),
        None => (None, TokenSource::None),
    }
}

/// Resolve the Bearer token to send: the developer's own if set, else the
/// shared FREE-TOKEN trial credential. `None` only when the free rung is
/// disabled AND no personal token exists.
pub fn token(project_dir: &Path) -> Option<String> {
    resolve(personal_token(project_dir), free_token()).0
}

/// Which credential `token()` resolved to (Personal | Free | None).
pub fn token_source(project_dir: &Path) -> TokenSource {
    resolve(personal_token(project_dir), free_token()).1
}

/// True when the developer configured their OWN token (not the free trial).
/// Drives the dev-admin "Configured ✓ / Free trial" status — the free token
/// being present must NOT read as "the developer is set up".
pub fn has_personal_token(project_dir: &Path) -> bool {
    personal_token(project_dir).is_some()
}

/// True when `candidate` is the shared FREE-TOKEN currently in effect. Used to
/// gate the dev-email attribution header — we only tell the server WHO is on
/// the shared trial, never leak the email alongside a personal/registered token.
pub fn is_free_token(candidate: &str) -> bool {
    free_token().as_deref() == Some(candidate.trim())
}

/// HTTP header carrying the developer's identity for shared-FREE-TOKEN calls,
/// so the server can rate-limit per person and invite them to register.
pub const DEV_EMAIL_HEADER: &str = "X-Tina4-Dev-Email";

/// Cached dev-email resolution — a `git config user.email` subprocess should
/// run at most once per agent process, not per grounding call.
static DEV_EMAIL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Pure choice of the dev email from its two candidates (env override wins,
/// then the git-config value), so the resolution is unit-testable without a
/// real git subprocess or global env. Blank/whitespace → treated as absent.
fn dev_email_from(env_override: Option<&str>, git_value: Option<&str>) -> Option<String> {
    for candidate in [env_override, git_value] {
        if let Some(v) = candidate {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The developer's email for FREE-TOKEN attribution: `TINA4_DEV_EMAIL` env
/// override, else `git config user.email` (the GitHub commit email). `None`
/// when neither exists — the header is simply omitted then. Resolved once and
/// cached; the agent serves a single project, so this is effectively a constant.
pub fn dev_email() -> Option<String> {
    DEV_EMAIL
        .get_or_init(|| {
            let env_override = std::env::var("TINA4_DEV_EMAIL").ok();
            let git_value = std::process::Command::new("git")
                .args(["config", "user.email"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            dev_email_from(env_override.as_deref(), git_value.as_deref())
        })
        .clone()
}

/// Attach the dev-email attribution header to a request — but ONLY when `token`
/// is the shared FREE-TOKEN and we have an email. On a personal/registered
/// token this is a no-op, so a registered developer's email is never sent.
fn with_dev_email(req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    if is_free_token(token) {
        if let Some(email) = dev_email() {
            return req.header(DEV_EMAIL_HEADER, email);
        }
    }
    req
}

/// Read a single `KEY=VALUE` from the project `.env`. Deliberately tiny — the
/// agent only needs one key, so we don't pull in a dotenv dependency (zero-dep
/// discipline). Ignores comments and surrounding quotes/whitespace.
pub fn read_env_file_value(project_dir: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(project_dir.join(".env")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches('"').trim_matches('\'').trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Persist the token to the project `.env` (upsert the `TINA4_MCP_TOKEN` line,
/// preserving every other line). Used by the dev-admin token-entry endpoint so
/// the developer never hand-edits `.env`. Creates the file if absent. Returns
/// the last-4 of the saved token for a masked confirmation.
pub fn save_token(project_dir: &Path, new_token: &str) -> std::io::Result<String> {
    let new_token = new_token.trim();
    let env_path = project_dir.join(".env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();

    let line = format!("{TOKEN_VAR}={new_token}");
    let mut replaced = false;
    let mut out: Vec<String> = Vec::new();
    for l in existing.lines() {
        if l.trim_start().starts_with(&format!("{TOKEN_VAR}=")) {
            out.push(line.clone());
            replaced = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !replaced {
        out.push(line);
    }
    let mut body = out.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    std::fs::write(&env_path, body)?;

    let last4: String = new_token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    Ok(last4)
}

// ── Grounding call ────────────────────────────────────────────────

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(MCP_TIMEOUT_SECS))
        .build()
        .expect("reqwest client build failed")
}

/// Retrieve framework grounding for `instruction` in `language`
/// (python/php/nodejs/ruby) from mcp.tina4.com's `tina4_context` tool.
///
/// Returns `Vec<RagHit>` — one hit per retrieved section — so the existing
/// coder-grounding + citation machinery (`format_hits_for_prompt`,
/// `verify_coder_grounding`) consumes it unchanged. Empty vec on ANY failure or
/// when no token is configured; the caller falls back to local `tina4-rag`.
pub async fn tina4_context(project_dir: &Path, instruction: &str, language: &str) -> Vec<RagHit> {
    let Some(tok) = token(project_dir) else {
        return Vec::new(); // unconfigured — caller degrades to tina4-rag
    };
    if instruction.trim().is_empty() {
        return Vec::new();
    }

    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "tina4_context",
            "arguments": { "instruction": instruction, "language": language }
        }
    });

    let req = http_client()
        .post(endpoint())
        .header("Authorization", format!("Bearer {tok}"))
        // Streamable-HTTP servers may answer with JSON or an SSE frame; accept both.
        .header("Accept", "application/json, text/event-stream");
    let resp = match with_dev_email(req, &tok).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[mcp] tina4_context send failed: {e}");
            return Vec::new();
        }
    };

    if !resp.status().is_success() {
        eprintln!("[mcp] tina4_context returned {}", resp.status());
        return Vec::new();
    }

    let raw = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[mcp] tina4_context body read failed: {e}");
            return Vec::new();
        }
    };

    let Some(text) = extract_tool_text(&raw) else {
        eprintln!("[mcp] tina4_context response had no tool text");
        return Vec::new();
    };
    parse_context_into_hits(&text, language)
}

/// Query the mcp.tina4.com **`long_context`** tool — a large-context reasoning
/// model that answers `question` given a (potentially huge) `context`. This is
/// the reasoning model behind the `thinking` slot when no Anthropic key is set
/// (see `agent::load_chat_settings`).
///
/// Takes the mcp base URL + Bearer token explicitly (rather than resolving from
/// `project_dir`) so the LLM path — which only holds a resolved `ModelSettings`,
/// not a project dir — can call it. Returns the answer text, or `None` on ANY
/// failure / missing token; the caller surfaces a clear error (there is no
/// secondary chat endpoint to fall back to). Uses a long timeout
/// (`LONG_CONTEXT_TIMEOUT_SECS`) since reasoning over a large context is slow.
/// Split a `long_context` tool response into `(answer, checksum)`.
///
/// The server appends a trailer to every answer:
/// `<answer>\n\n---\nchecksum: cx_<hex>  (…)`. Return the answer with that trailer
/// stripped and the `cx_…` token when present. Pure string work — no deps. Only
/// splits when a real `cx_…` token follows the marker, so an answer that merely
/// mentions "checksum" is never truncated.
pub(crate) fn split_checksum(text: &str) -> (String, Option<String>) {
    if let Some(marker) = text.rfind("---\nchecksum:") {
        if let Some(cx) = text[marker..]
            .split_whitespace()
            .find(|t| t.starts_with("cx_"))
        {
            return (text[..marker].trim_end().to_string(), Some(cx.to_string()));
        }
    }
    (text.to_string(), None)
}

/// Call the `long_context` tool. Sends only the NEW `context` chunk plus the prior
/// `checksum` (both optional) so the accumulated corpus is never resent, and
/// returns `(clean answer, new checksum)` with the checksum trailer stripped.
pub async fn long_context_call(
    base_url: &str,
    token: &str,
    question: &str,
    context: &str,
    checksum: &str,
) -> Option<(String, String)> {
    if token.trim().is_empty() || question.trim().is_empty() {
        return None;
    }
    let url = format!("{}/mcp", base_url.trim_end_matches('/'));

    let mut arguments = serde_json::Map::new();
    arguments.insert("question".into(), json!(question));
    if !context.is_empty() {
        arguments.insert("context".into(), json!(context));
    }
    if !checksum.is_empty() {
        arguments.insert("checksum".into(), json!(checksum));
    }
    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "long_context",
            "arguments": arguments
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(LONG_CONTEXT_TIMEOUT_SECS))
        .build()
        .ok()?;

    let req = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream");
    let resp = match with_dev_email(req, token).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[mcp] long_context send failed: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        eprintln!("[mcp] long_context returned {}", resp.status());
        return None;
    }
    let raw = resp.text().await.ok()?;
    let text = extract_tool_text(&raw)?;
    let (answer, checksum) = split_checksum(&text);
    if answer.trim().is_empty() {
        None
    } else {
        Some((answer, checksum.unwrap_or_default()))
    }
}

/// Query the mcp.tina4.com **`tina4_chat`** tool — the fine-tuned Tina4 coder.
/// Unlike `long_context` (a general Q&A model that summarizes code onto one
/// line), this is code-oriented and Tina4-aware, so it reliably emits the
/// multi-file `## FILE:` blocks the coder loop parses. `messages` is an
/// OpenAI-format array (`[{role, content}, …]`); the assistant's next reply is
/// returned. Grounding (`tina4_context`) is still injected upstream by
/// `ground_coder_msg`, so this is "Tina4 context + Tina4 chat" together.
///
/// Returns the reply text, or `None` on any failure / missing token.
pub async fn tina4_chat_call(base_url: &str, token: &str, messages: serde_json::Value) -> Option<String> {
    if token.trim().is_empty() {
        return None;
    }
    let url = format!("{}/mcp", base_url.trim_end_matches('/'));
    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "tina4_chat", "arguments": { "messages": messages } }
    });

    // Code generation can be slow; reuse the long-context timeout.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(LONG_CONTEXT_TIMEOUT_SECS))
        .build()
        .ok()?;

    let req = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream");
    let resp = match with_dev_email(req, token).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[mcp] tina4_chat send failed: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        eprintln!("[mcp] tina4_chat returned {}", resp.status());
        return None;
    }
    let raw = resp.text().await.ok()?;
    let text = extract_tool_text(&raw)?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Pull the tool result text out of a JSON-RPC response body, tolerating both
/// plain JSON and SSE framing (`event: ...\ndata: {json}\n\n`). Returns the
/// concatenated `result.content[*].text`. Returns None on a JSON-RPC error or
/// an unparseable body (caller logs + degrades).
fn extract_tool_text(raw: &str) -> Option<String> {
    // Plain JSON first (the common case for a single tools/call response).
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return tool_text_from_value(&v);
    }
    // SSE fallback: find the last `data:` line that parses into a JSON-RPC
    // result. Streamable-HTTP emits one or more `data:` frames.
    let mut found: Option<String> = None;
    for line in raw.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("data:") {
            let payload = rest.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(t) = tool_text_from_value(&v) {
                    found = Some(t); // keep the latest complete result frame
                }
            }
        }
    }
    found
}

/// Extract `result.content[*].text` from a parsed JSON-RPC value. None if the
/// value carries an `error` or lacks a text content block.
fn tool_text_from_value(v: &serde_json::Value) -> Option<String> {
    if v.get("error").is_some() {
        if let Some(msg) = v["error"]["message"].as_str() {
            eprintln!("[mcp] tina4_context error: {msg}");
        }
        return None;
    }
    let content = v.get("result")?.get("content")?.as_array()?;
    let mut out = String::new();
    for block in content {
        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Split the retrieved grounding markdown into one `RagHit` per `### ` section
/// so each is separately citable (`# grounded-by: [N]`). If there are no `### `
/// headers, the whole text becomes a single hit. A leading preamble before the
/// first header is dropped — it's boilerplate ("Retrieved … API + examples"),
/// not a citable pattern.
fn parse_context_into_hits(text: &str, language: &str) -> Vec<RagHit> {
    let mut hits: Vec<RagHit> = Vec::new();
    // Sections start at a line beginning with "### ".
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();

    let flush = |title: &Option<String>, body: &str, hits: &mut Vec<RagHit>| {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        hits.push(RagHit {
            text: body.to_string(),
            metadata: RagMetadata {
                title: title.clone().unwrap_or_default(),
                source: "mcp.tina4.com".into(),
                url: "https://mcp.tina4.com".into(),
                language: language.to_string(),
                chunk_index: hits.len() as u32,
            },
            distance: 0.0,
        });
    };

    for line in text.lines() {
        if let Some(h) = line.strip_prefix("### ") {
            // New section — flush the previous one (unless it's the preamble,
            // i.e. current_title is None and we haven't seen a header yet).
            if current_title.is_some() {
                flush(&current_title, &current_body, &mut hits);
            }
            current_title = Some(h.trim().to_string());
            current_body.clear();
        } else if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if current_title.is_some() {
        flush(&current_title, &current_body, &mut hits);
    }

    // No `### ` sections at all — treat the entire text as one hit.
    if hits.is_empty() {
        let body = text.trim();
        if !body.is_empty() {
            hits.push(RagHit {
                text: body.to_string(),
                metadata: RagMetadata {
                    title: "tina4_context".into(),
                    source: "mcp.tina4.com".into(),
                    url: "https://mcp.tina4.com".into(),
                    language: language.to_string(),
                    chunk_index: 0,
                },
                distance: 0.0,
            });
        }
    }
    hits
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn endpoint_appends_mcp_path() {
        // Default when unset.
        assert_eq!(endpoint(), "https://mcp.tina4.com/mcp");
    }

    #[test]
    fn split_checksum_strips_trailer_and_extracts_token() {
        let raw = "X is 42.\n\n---\nchecksum: cx_4c93a72dac1c54c238aabb42c5da7570  (pass back as `checksum` to append more context or re-query — accumulated 39 chars over 1 chunk(s))";
        let (answer, cs) = split_checksum(raw);
        assert_eq!(answer, "X is 42.");
        assert_eq!(cs.as_deref(), Some("cx_4c93a72dac1c54c238aabb42c5da7570"));
    }

    #[test]
    fn split_checksum_no_trailer_returns_text_unchanged() {
        let (answer, cs) = split_checksum("Just an answer, no trailer.");
        assert_eq!(answer, "Just an answer, no trailer.");
        assert_eq!(cs, None);
    }

    #[test]
    fn split_checksum_ignores_inline_mention_without_token() {
        // The word "checksum" appears but there is no `---\nchecksum: cx_…` trailer.
        let text = "To verify, compare the checksum of each file.";
        let (answer, cs) = split_checksum(text);
        assert_eq!(answer, text);
        assert_eq!(cs, None);
    }

    #[test]
    fn split_checksum_uses_the_last_marker() {
        // An answer that itself shows an example trailer, then the real one.
        let raw = "Example: ---\nchecksum: cx_deadbeef\nNow the real answer.\n\n---\nchecksum: cx_final0001  (…)";
        let (answer, cs) = split_checksum(raw);
        assert!(answer.ends_with("Now the real answer."));
        assert_eq!(cs.as_deref(), Some("cx_final0001"));
    }

    /// Real wire round-trip against mcp.tina4.com. `#[ignore]`d so the normal
    /// suite never hits the network; run explicitly with a token:
    ///   TINA4_MCP_TOKEN=… cargo test wire_long_context -- --ignored --nocapture
    #[test]
    #[ignore]
    fn wire_long_context_store_then_requery() {
        let Ok(token) = std::env::var("TINA4_MCP_TOKEN") else {
            eprintln!("skip: TINA4_MCP_TOKEN not set");
            return;
        };
        let base = base_url();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // 1) Store context, get a checksum. Answer must be CLEAN (trailer stripped).
            let (a1, c1) = long_context_call(
                &base, &token,
                "What number is X?",
                "X is 99. wire-test-alpha marker.",
                "",
            ).await.expect("first call failed");
            assert!(a1.contains("99"), "answer should mention 99, got: {a1}");
            assert!(c1.starts_with("cx_"), "expected a cx_ checksum, got: {c1}");
            assert!(!a1.contains("checksum:"), "trailer leaked into answer: {a1}");

            // 2) Re-query with the checksum ALONE (no context resent) — the server
            //    answers from the stored corpus and returns the SAME checksum.
            let (a2, c2) = long_context_call(
                &base, &token, "What number is X?", "", &c1,
            ).await.expect("requery failed");
            assert!(a2.contains("99"), "requery lost the stored context, got: {a2}");
            assert_eq!(c2, c1, "re-query (no new context) must keep the same checksum");
        });
    }

    #[test]
    fn extract_text_from_plain_json() {
        let raw = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"### a\\ncode\"}]}}";
        assert_eq!(extract_tool_text(raw).as_deref(), Some("### a\ncode"));
    }

    #[test]
    fn extract_text_from_sse_frame() {
        let raw = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n\n";
        assert_eq!(extract_tool_text(raw).as_deref(), Some("hello"));
    }

    #[test]
    fn extract_text_returns_none_on_jsonrpc_error() {
        let raw = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"Unauthorized"}}"#;
        assert_eq!(extract_tool_text(raw), None);
    }

    #[test]
    fn parse_context_splits_by_section() {
        let text = "Retrieved preamble to drop\n### file/one.ts\n```ts\nA\n```\n### file/two.ts\n```ts\nB\n```";
        let hits = parse_context_into_hits(text, "nodejs");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].metadata.title, "file/one.ts");
        assert_eq!(hits[0].metadata.chunk_index, 0);
        assert!(hits[0].text.contains('A'));
        assert!(!hits[0].text.contains("preamble")); // preamble dropped
        assert_eq!(hits[1].metadata.title, "file/two.ts");
        assert_eq!(hits[1].metadata.chunk_index, 1);
        assert_eq!(hits[1].metadata.language, "nodejs");
    }

    #[test]
    fn parse_context_single_hit_when_no_sections() {
        let hits = parse_context_into_hits("just some text, no headers", "python");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].metadata.source, "mcp.tina4.com");
    }

    #[test]
    fn token_prefers_process_env_then_env_file() {
        let dir = std::env::temp_dir().join(format!("tina4_mcp_tok_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(".env"), "TINA4_MCP_TOKEN=from_file\nOTHER=1\n").unwrap();
        // No process env set for this key in the test → reads the file.
        // (We avoid mutating the real process env to keep the test hermetic.)
        assert_eq!(read_env_file_value(&dir, "TINA4_MCP_TOKEN").as_deref(), Some("from_file"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_token_upserts_and_preserves_other_lines() {
        let dir = std::env::temp_dir().join(format!("tina4_mcp_save_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(".env"), "FOO=bar\nTINA4_MCP_TOKEN=old\nBAZ=qux\n").unwrap();
        let last4 = save_token(&dir, "abcd1234567").unwrap();
        assert_eq!(last4, "4567");
        let body = fs::read_to_string(dir.join(".env")).unwrap();
        assert!(body.contains("FOO=bar"));
        assert!(body.contains("BAZ=qux"));
        assert!(body.contains("TINA4_MCP_TOKEN=abcd1234567"));
        assert!(!body.contains("TINA4_MCP_TOKEN=old"));
        // Exactly one token line.
        assert_eq!(body.matches("TINA4_MCP_TOKEN=").count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_token_appends_when_absent() {
        let dir = std::env::temp_dir().join(format!("tina4_mcp_app_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(".env"), "FOO=bar\n").unwrap();
        save_token(&dir, "newtoken").unwrap();
        let body = fs::read_to_string(dir.join(".env")).unwrap();
        assert!(body.contains("FOO=bar"));
        assert!(body.contains("TINA4_MCP_TOKEN=newtoken"));
        let _ = fs::remove_dir_all(&dir);
    }

    // ── FREE-TOKEN trial resolution ───────────────────────────────────
    // The resolution ORDER lives in the pure `resolve`/`free_token_from`
    // helpers, so these exercise the real logic without mutating the global
    // process environment (which would race the parallel test runner).

    #[test]
    fn resolve_prefers_personal_over_free() {
        let (tok, src) = resolve(Some("mine".into()), Some("FREE-TOKEN".into()));
        assert_eq!(tok.as_deref(), Some("mine"));
        assert_eq!(src, TokenSource::Personal);
    }

    #[test]
    fn resolve_falls_back_to_free_when_no_personal() {
        let (tok, src) = resolve(None, Some("FREE-TOKEN".into()));
        assert_eq!(tok.as_deref(), Some("FREE-TOKEN"));
        assert_eq!(src, TokenSource::Free);
    }

    #[test]
    fn resolve_blank_personal_is_ignored_falls_to_free() {
        // An empty `TINA4_MCP_TOKEN=` line must not mask the free trial.
        let (tok, src) = resolve(Some("   ".into()), Some("FREE-TOKEN".into()));
        assert_eq!(tok.as_deref(), Some("FREE-TOKEN"));
        assert_eq!(src, TokenSource::Free);
    }

    #[test]
    fn resolve_none_when_free_disabled_and_no_personal() {
        let (tok, src) = resolve(None, None);
        assert_eq!(tok, None);
        assert_eq!(src, TokenSource::None);
    }

    #[test]
    fn free_token_defaults_to_the_literal_constant() {
        // Out of the box (no TINA4_FREE_TOKEN override) → the shipped FREE-TOKEN.
        assert_eq!(free_token_from(None).as_deref(), Some("FREE-TOKEN"));
    }

    #[test]
    fn free_token_env_override_wins() {
        assert_eq!(free_token_from(Some("t4_rotated")).as_deref(), Some("t4_rotated"));
    }

    #[test]
    fn free_token_blank_override_disables_the_free_rung() {
        // Deploying with TINA4_FREE_TOKEN="" turns the trial off (→ None).
        assert_eq!(free_token_from(Some("   ")), None);
    }

    #[test]
    fn personal_token_reads_from_env_file_when_no_process_env() {
        // Hermetic: no process env set for the key → resolves from .env, and
        // that counts as a personal (not free) credential.
        let dir = std::env::temp_dir().join(format!("tina4_mcp_pers_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(".env"), "TINA4_MCP_TOKEN=t4_dev_own\n").unwrap();
        assert_eq!(personal_token(&dir).as_deref(), Some("t4_dev_own"));
        // With a personal token present, resolve() must pick it over free.
        let (tok, src) = resolve(personal_token(&dir), Some("FREE-TOKEN".into()));
        assert_eq!(tok.as_deref(), Some("t4_dev_own"));
        assert_eq!(src, TokenSource::Personal);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_personal_token_is_false_on_a_bare_project() {
        // No .env, no personal token → free trial territory (nudge to register).
        let dir = std::env::temp_dir().join(format!("tina4_mcp_bare_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let _ = fs::remove_file(dir.join(".env"));
        assert!(!has_personal_token(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    // ── Dev-email attribution (FREE-TOKEN only) ───────────────────────

    #[test]
    fn is_free_token_matches_the_literal_only() {
        // With no TINA4_FREE_TOKEN override, the free token is the literal.
        assert!(is_free_token("FREE-TOKEN"));
        assert!(is_free_token("  FREE-TOKEN  ")); // trimmed
        assert!(!is_free_token("t4_a_personal_token"));
        assert!(!is_free_token(""));
    }

    #[test]
    fn dev_email_prefers_env_override_then_git() {
        assert_eq!(dev_email_from(Some("me@dev.io"), Some("git@x.io")).as_deref(), Some("me@dev.io"));
    }

    #[test]
    fn dev_email_falls_back_to_git_config() {
        assert_eq!(dev_email_from(None, Some("git@x.io")).as_deref(), Some("git@x.io"));
    }

    #[test]
    fn dev_email_blank_candidates_are_absent() {
        // No email anywhere, or only whitespace → header is omitted (None).
        assert_eq!(dev_email_from(None, None), None);
        assert_eq!(dev_email_from(Some("  "), Some("")), None);
    }

    /// Real localhost round-trip: capture the raw HTTP request long_context_call
    /// emits and assert the X-Tina4-Dev-Email header is present on the shared
    /// FREE-TOKEN and ABSENT on a personal token. No mock — a real TcpListener
    /// reads real bytes reqwest put on the wire.
    #[tokio::test]
    async fn dev_email_header_rides_only_the_free_token() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // Only TINA4_DEV_EMAIL is set (no other test reads it); the free token
        // stays the literal default so we don't disturb is_free_token's test.
        unsafe { std::env::set_var("TINA4_DEV_EMAIL", "dev@tina4.test"); }

        async fn capture_request(token: &str) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(), body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                req
            });
            let base = format!("http://{addr}");
            let _ = long_context_call(&base, token, "q?", "ctx", "").await;
            server.await.unwrap()
        }

        let free_req = capture_request("FREE-TOKEN").await;
        assert!(
            free_req.to_lowercase().contains("x-tina4-dev-email: dev@tina4.test"),
            "free-token request must carry the dev-email header, got:\n{free_req}"
        );

        let personal_req = capture_request("t4_a_personal_token").await;
        assert!(
            !personal_req.to_lowercase().contains("x-tina4-dev-email"),
            "personal-token request must NOT carry the dev-email header, got:\n{personal_req}"
        );
    }
}
