# Task: implement the long_context checksum (append-not-resend) in the supervisor

## Goal
The mcp.tina4.com `long_context` tool now returns a `checksum` per response and
supports append/re-query so callers never resend the accumulated corpus. Wire the
supervisor to use it: send stable context once, then only the per-turn delta +
the checksum. Cuts payload/calls; behaviour-preserving (transport-only).

## Server contract (verified live, 2026-07-28)
- Request `arguments`: `{question, context?, checksum?}` (>=1 of context/checksum).
  - First: `{question, context}` → stores corpus, returns a `cx_…` checksum.
  - Append: `{question, context:<delta>, checksum:<prev>}` → new checksum.
  - Re-query: `{question, checksum:<prev>}` (no context) → same checksum.
- Response text = `<answer>\n\n---\nchecksum: cx_<hex>  (…)`. Parse `cx_…`, strip trailer.
- Checksums are private to the token; corpus accumulates server-side.

## Current state (bug + miss)
`src/agent.rs::llm_call` (long_context branch, ~2373) rebuilds `context =
system_prompt + all messages` and sends it EVERY call via
`mcp_context::long_context_call(url,key,question,context)`, which returns the raw
text INCLUDING the new `---\nchecksum:` trailer.
1. 🐛 Every long_context answer is polluted with the trailer (leaks into plans).
2. 💸 Full context re-sent every turn.

## Design (option A — per-thread cache)
### Phase 1 — parse + strip (correctness, all callers)
- `mcp_context`: `split_checksum(text) -> (answer, Option<checksum>)` (pure, no deps,
  string ops). `long_context_call` gains a `checksum: &str` arg, sends it when
  non-empty, and returns `Option<(String, String)>` = (clean answer, new checksum).
- `llm_call`'s long_context branch uses the stripped answer (passes checksum="" →
  full send, current behaviour but CLEAN). Zero caller changes.

### Phase 2 — thread the checksum (limit calls)
- Module cache: `static LONG_CONTEXT_CACHE: OnceLock<Mutex<HashMap<String, Chain>>>`
  (mirrors the existing `FEEDBACK_CONVOS`). `Chain { checksum, sent_len, prefix_hash }`.
- `llm_call_cached(settings, system, msgs, max, temp, cache_key)`:
  - key present + prefix matches (`hash(system + msgs[..sent_len]) == prefix_hash`
    and `msgs.len() >= sent_len`):
      - delta empty → re-query (context="", checksum).
      - else → append (context = delta msgs only, NO system prompt, + checksum).
      - update chain (checksum, sent_len=msgs.len(), prefix_hash over full).
  - miss / prefix mismatch (edited/reset history, changed system prompt) →
    INVALIDATE: full send (system + all msgs, checksum=""), store fresh chain.
  - non-mcp models → delegate to `llm_call` unchanged.
- Switch only the per-thread turn-loop callers to `llm_call_cached` with
  `cache_key = "{thread_id}:{purpose}"` (reasoning/planner/coder/debug). One-off
  callers (human_message, feedback reply, tests) stay on `llm_call`.

Correctness rests on: the corpus grows by appending, so accumulated == intended
context; the prefix_hash guard forces a full resend whenever that's not true.

## Tests (real, no mocks — pure logic)
- `split_checksum`: trailer present → (clean answer, cx_…); absent → (text, None);
  answer with an inline "checksum" word not at the trailer → not mis-split.
- delta decision (`plan_send(system, msgs, cached) -> Send::{Full|Append|Requery}`):
  first call → Full; +1 msg, prefix matches → Append(delta); no new msg → Requery;
  edited prefix → Full (invalidate); shorter msgs → Full.
- `cargo test` + `cargo clippy` green; `cargo build --release`.
- Real wire check: the contract was already confirmed via live tool calls (append
  grows the corpus; checksum-only re-query returns the stored answer).

## Scope / branch
Supervisor only (the Rust client of long_context). tina4 CLI repo, branch
`feature/long-context-checksum` off `main`. Framework repos don't call
long_context directly — no parity port. Workers: n/a (main session implements).

## Status: 🟡 building
