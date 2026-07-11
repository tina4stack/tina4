# Task: Wire the two MCPs into the agent coder (supervisor-first)

> Branch context: **feature branch off the CLI release line (`main`)** — the
> `tina4` binary is shared by all four frameworks, so this is a single-codebase
> change (parity is automatic for the agent loop). Confirm branch name with the
> maintainer before cutting.

## Goal

Teach the Rust supervisor's specialist agents (coder, planner — and optionally
supervisor) to ground themselves in **version-current framework API** by calling
`mcp.tina4.com`'s `tina4_context(instruction, language)` before writing Tina4
code — instead of relying solely on the static, drift-prone examples baked into
the agent system prompts.

## Context — two MCPs, two jobs

| MCP | Role | Wired today? |
|---|---|---|
| **Local dev MCP** (`/__dev/mcp` on framework port) | Project actions: files, routes, DB, plans, index | ✅ agent already uses it (server-side) + `.mcp.json` for Claude Code |
| **Official `mcp.tina4.com`** (tina4-coder) | Framework grounding: `tina4_context(instruction, language)` — live corpus | ❌ **not referenced anywhere** in `agent.rs` |

Framework grounding today lives as hardcoded examples inside `DEFAULT_AGENTS`
prompts in `src/agent.rs` (and a dead Python-only `tina4-context.ts` in the SPA).
`tina4_context` is retrieval-only, language-correct, and updates instantly with
the corpus — the authoritative replacement. Verified live: a `language="nodejs"`
call returns real `BaseModel` / `static fields` / auto-CRUD idioms.

## Design

1. **New agent tool `tina4_context`** in the Rust agent tool registry, added to
   the `coder` and `planner` tool allowlists (supervisor optional). Gives the
   model agency to ground itself — mirrors the existing "call `docs_search`
   before guessing" idiom already in the prompts.
2. **Execution (Rust):** POST JSON-RPC `tools/call` to `${TINA4_MCP_URL}` with
   `Authorization: Bearer ${TINA4_MCP_TOKEN}`; `language` filled from
   `detect_language()` (never trust the model to pass it). Reuse the existing
   `reqwest::Client`.
3. **Config (env, `TINA4_*` convention):**
   - `TINA4_MCP_URL` — default `https://mcp.tina4.com`.
   - `TINA4_MCP_TOKEN` — Bearer token (free at profile.tina4.com).
   - **Token entry in the dev admin (required by maintainer):** a masked field
     in the SPA where the developer pastes the token. On save it is persisted as
     `TINA4_MCP_TOKEN` in the project `.env` (gitignored). The shared Rust agent
     owns the write (single endpoint, all four frameworks) and resolves the token
     at call time from **process env → project `.env` fallback**, so a
     freshly-entered token works without restarting the agent. Never echoed back
     in full; UI shows only a "configured ✓ / not set" state + last 4 chars.
4. **Graceful degradation (never break the loop):** token/URL unset or host
   unreachable → the tool returns a clear "framework grounding unavailable —
   proceeding from baked-in examples" message; the agent continues on the
   static prompts. No hard failure.
5. **Caching:** memoise per `(sha(instruction), language)` for the session so a
   multi-round coder turn doesn't re-hit the endpoint.
6. **Prompt update:** teach `coder`/`planner` to call `tina4_context` before
   emitting framework code. (Do NOT delete the baked-in examples in this pass —
   they are the fallback. Shrinking them is a follow-up once grounding is proven.)

## Scope
- [x] Read the coder/planner grounding path in `agent.rs` — found the existing
      auto-inject pipeline (`ground_coder_msg` → `rag::search` → citation rule).
      **The right integration was auto-inject, not a model-called tool** — the
      codebase already grounds every coder turn; it just used local tina4-rag.
- [x] Add `mcp.tina4.com` grounding: new `src/mcp_context.rs` (reqwest → JSON-RPC
      `tools/call`), returns `Vec<RagHit>` so the existing citation/verify
      machinery is unchanged. `ground_coder_msg` now prefers it, falls back to RAG.
- [x] Config: `TINA4_MCP_URL` / `TINA4_MCP_TOKEN` registered in `env_config.rs`
      (surfaces in `.env.example`).
- [x] Language from `tina4_context_language(files)` → `detect_language()` fallback.
- [x] Graceful degradation: no token / unreachable → empty hits → RAG fallback,
      loop never breaks (unit-tested).
- [ ] ~~Session cache~~ — deferred: `ground_coder_msg` fires once per coder turn,
      not per tool-round (no generic tool loop), so within-turn caching is moot.
      Revisit only if plan-execution starts re-grounding per step in a tight loop.
- [ ] Update `coder` + `planner` system prompts to mention `tina4_context` as the
      grounding source (currently the injection is silent + citation-enforced;
      prompt copy still says "tina4-rag" in places — low priority, cosmetic).
- [x] **Token entry surface:** agent `GET /mcp/status` + `POST /mcp/token`
      (upserts `.env`), dev-admin 🔑 panel in the Threads header, vite proxy
      (`/__dev/api/grounding/*` → agent `/mcp/*`), node backend proxy routes.
- [x] Tests (below) — real endpoint + real agent process, no mocks

## Parity
The `tina4` binary is shared across all frameworks — one change covers all four.

| Feature | (single Rust binary) |
|---|---|
| `tina4_context` grounding source | ✅ Done |
| Config + graceful degradation | ✅ Done |
| Agent token endpoints (`/mcp/status`, `/mcp/token`) | ✅ Done |

### Dev-admin + framework backend proxy parity
The token panel needs `/__dev/api/grounding/{status,token}` proxied to the agent:

| Layer | Status |
|---|---|
| dev-admin SPA panel + vite proxy | ✅ Done (verified in browser) |
| tina4-nodejs backend proxy | ✅ Done (typecheck green) |
| tina4-python backend proxy | ❌ BUILD (parity) |
| tina4-php backend proxy | ❌ BUILD (parity) |
| tina4-ruby backend proxy | ❌ BUILD (parity) |

## Tests (real — no mocks, positive + negative)
- [x] **Response parsing** — plain-JSON + SSE-framed JSON-RPC → tool text;
      JSON-RPC error → None; section-splitting into citable hits. (9 unit tests,
      green.)
- [x] **`.env` token I/O** — `save_token` upserts + preserves other lines +
      appends when absent; `token()` reads process env → `.env`. (green.)
- [x] **Real agent process** — built the binary, ran `tina4 agent`, exercised
      `GET /mcp/status` (false → true) + `POST /mcp/token` (writes real `.env`) +
      empty-token `400`. No mocks — real process, real file.
- [x] **Full Rust suite** — 95 passed / 0 failed / clippy clean at HEAD (macOS).
- [ ] **Live authenticated `tina4_context`** — GATED on a real `TINA4_MCP_TOKEN`.
      The tool itself is verified live via the harness (returned real nodejs
      grounding) and the wire protocol confirmed (401 w/o token, `/mcp` endpoint);
      the in-agent authenticated call runs once the maintainer provides a token.

## Bugs
- [ ] (log here as [ ], tick when a real test proves it fixed)

## Commits
- (hash  description — one per landed change)

## Verification honesty (must-read)
The full agent loop cannot be verified end-to-end **on this machine**: the
default Tina4 Cloud model host (`41.71.84.173:11437`) is unreachable here
(`/__dev/api/chat` returns `"No models available"`). So end-to-end will be
qualified: verify the `tina4_context` executor in isolation (real endpoint +
Rust tests) and, for the loop, use the `ANTHROPIC_API_KEY` path (Claude) which
`agent.rs` already supports — OR run on a host with Tina4 Cloud reachable. Every
claim will be scoped to where it was actually run.

## Addendum — `long_context` IS the thinking model (no Anthropic key)

The official MCP added a **`long_context`** tool (`question` + `context` Q&A,
~millions of tokens, *untainted by Tina4*). The old Tina4 Cloud chat endpoints
(qwen @ 41.71.84.173) are **retired** — there is no local chat model anymore.
So the model topology is now just two providers:

    thinking = Claude Sonnet   (if ANTHROPIC_API_KEY is set)
             = long_context    (otherwise, via mcp.tina4.com — needs TINA4_MCP_TOKEN)

Wired as a first-class provider so EVERY agent on the `thinking` slot rides it —
supervisor, planner, **coder (still grounded via `ground_coder_msg` →
`tina4_context`)**, debug, intake. Tina4 correctness comes from grounding, not
from the model (which is Tina4-agnostic). Implementation:

- `mcp_context::long_context_call(base_url, token, question, context)` — explicit
  base+token so `llm_call` (no `project_dir`) can call it; 300s timeout.
- `load_chat_settings()` — no key → `thinking` = `ModelSettings{provider:"tina4-mcp",
  model:"long_context", url:<TINA4_MCP_URL>, api_key:<token from env/.env>}`.
- `llm_call()` — new `provider=="tina4-mcp"` branch maps the chat turn to the
  tool's question/context and returns its answer; **no qwen fallback** — a
  failure (no token / mcp down, no key) surfaces a clear error.
- Per-agent models and call sites reverted to plain `"thinking"` / `llm_call`;
  the earlier `reasoning_llm_call` wrapper was removed (routing lives in
  `llm_call` now — zero call-site special-casing).

Verified (macOS, real agent process, no mocks):
- [x] Live `long_context` call returns a coherent answer (real token).
- [x] No key + token → supervisor `/chat` logs `[llm] tina4-mcp long_context`
      and answers; its JSON action still parses (`kind=respond`).
- [x] No key + no token → clean error, no phantom qwen call.
- [x] 95 Rust tests pass, clippy clean.

NOTE / follow-up: `vision` + `image_gen` slots still reference the retired Tina4
Cloud topology. Image gen is the `tina4_image` MCP tool (not wired to the chat
path); there is no vision tool yet, so `vision` is a text-only long_context
placeholder. Neither is used by the code-building POC — flagged for a later pass.

## Addendum 2 — coder redesign for the independent (no-Claude) mechanism

Testing the tools proved: `tina4_chat` is the Python-tuned coder and emits
**correct, current tina4-python** — but as a *bare code block* (no `## FILE:`
header, no `# grounded-by:` citation), which the loop's post-processing rejected.
And it **regenerates** files (doesn't edit), so pre-scaffolding a skeleton just
tripped the write's anti-shrink guard.

Redesigned the `code`-action coder path (`agent.rs`) for the `tina4_chat` coder:
- `derive_coder_path(ctx, files)` — deterministic target path (supervisor's
  `files`, else the route in `context` → `src/routes/<name>.py`).
- Skip the citation-verify retry (`llm_call`, not `llm_call_with_grounding_retry`).
- Write tina4_chat's fresh output to the derived path (synthesize the `## FILE:`
  header so the shared writer runs); NO pre-scaffold (avoids the shrink guard).
- `run_framework_generate()` (`tina4 generate <kind> <name>`) kept as a **fallback**
  when tina4_chat produces nothing — framework-native skeleton still lands.

Verified end-to-end (macOS, real agent process, no mocks, no Anthropic key):
- [x] `/chat` → supervisor `kind=code` → coder `tina4_chat` → **`Written:
      src/routes/hello.py`** → **IMPORT OK** in the real `tina4_python` env.
- [x] 95 Rust tests pass, clippy clean.

Remaining: the **execute_plan** coder steps (2 other call sites at ~3077/~3500)
still use the old `## FILE:` + citation path — they need the same treatment for
plan-driven builds. And nodejs/php/ruby coders (tina4_chat is Python-only).

## Addendum 4 — RUNNABLE code, end-to-end (the goal)

Generate-first landed: the coder detects scaffoldable artifacts (resource/CRUD,
model, migration) and runs the framework generators — the textbook path — before
the LLM. `tina4_chat` authors only genuinely custom logic; plain routes are not
over-scaffolded. Steering: `TINA4_ESSENCE` now carries the "generators, not
hand-roll" rule for the reasoning agents.

Proven RUNNABLE (macOS, real `tina4python init` app, no Anthropic key, no mocks):
- [x] Agent builds "products resource (Product has name+price)" → 4 files:
      `src/orm/Product.py` + up/down migrations + secure CRUD `src/routes/products.py`.
- [x] `tina4python migrate` applies the migration (table created).
- [x] App boots (`Server started http://localhost:7146`, 8 routes, no errors).
- [x] `GET /api/products` → 200 `{"records":[],"count":0,"page":1,...}`.
- [x] Secure-by-default confirmed by the running framework: reads public,
      POST/PUT/DELETE `auth=required` — no `@noauth()` foot-guns.
- [x] Simple route ("GET /hello greeting") → tina4_chat minimal handler (not
      over-scaffolded); imports clean.
- [x] 95 Rust tests pass, clippy clean.

Committed on `feature/independent-coding-agent`.

## Status: RUNNABLE code proven (macOS, direct `code` path). Remaining:
##   execute_plan (plan-driven) coder-step parity, scaffold 3-framework parity,
##   tests-first in the loop, vision/image slots.
