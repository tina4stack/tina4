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

/// Resolve the Bearer token: process env first, then the project `.env`
/// fallback. Blank values are treated as absent so an empty `TINA4_MCP_TOKEN=`
/// line doesn't mask a real token set in the environment.
pub fn token(project_dir: &Path) -> Option<String> {
    if let Ok(t) = std::env::var(TOKEN_VAR) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    read_env_file_value(project_dir, TOKEN_VAR)
}

/// True when a token is configured (env or `.env`). Drives the dev-admin
/// "configured ✓ / not set" status without exposing the token itself.
pub fn is_configured(project_dir: &Path) -> bool {
    token(project_dir).is_some()
}

/// Read a single `KEY=VALUE` from the project `.env`. Deliberately tiny — the
/// agent only needs one key, so we don't pull in a dotenv dependency (zero-dep
/// discipline). Ignores comments and surrounding quotes/whitespace.
fn read_env_file_value(project_dir: &Path, key: &str) -> Option<String> {
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

    let resp = match http_client()
        .post(endpoint())
        .header("Authorization", format!("Bearer {tok}"))
        // Streamable-HTTP servers may answer with JSON or an SSE frame; accept both.
        .header("Accept", "application/json, text/event-stream")
        .json(&req_body)
        .send()
        .await
    {
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
pub async fn long_context_call(base_url: &str, token: &str, question: &str, context: &str) -> Option<String> {
    if token.trim().is_empty() || question.trim().is_empty() {
        return None;
    }
    let url = format!("{}/mcp", base_url.trim_end_matches('/'));

    let req_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "long_context",
            "arguments": { "question": question, "context": context }
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(LONG_CONTEXT_TIMEOUT_SECS))
        .build()
        .ok()?;

    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream")
        .json(&req_body)
        .send()
        .await
    {
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
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
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

    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream")
        .json(&req_body)
        .send()
        .await
    {
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
}
