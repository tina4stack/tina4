# Task: local reasoning-slot override for the dev-admin supervisor, with fallback

## Goal
Let the dev-admin's supervisor reason on a LOCAL OpenAI-compatible model
(e.g. Qwen2.5-7B-Instruct served as `ctx-reader` on the GPU host) instead of
mcp.tina4.com's `long_context`, and fall back to the mcp model when the local
endpoint is down. Config-only surface; the generic `llm_call` OpenAI path
already works (builds `{url}/v1/chat/completions`, no auth header when the key
is empty).

## Config (env, mirrors the existing ANTHROPIC_API_KEY override)
- `TINA4_LOCAL_MODEL_URL` — base URL of the local endpoint (e.g.
  `http://192.168.88.99:11460`; a trailing `/v1` is stripped so both forms work).
  When set, the reasoning (`thinking`) slot points here.
- `TINA4_LOCAL_MODEL` — model id (default `ctx-reader`).
- `TINA4_LOCAL_MODEL_KEY` — optional bearer/x-api-key (default empty = no auth).
- `TINA4_LOCAL_MODEL_FALLBACK` — `0`/`false` disables the fallback (default on).

## Design
- `ChatSettings` gains `reasoning_fallback: Option<ModelSettings>` (serde default,
  skip-if-none so settings.json still round-trips).
- `apply_local_reasoning_override(ChatSettings) -> ChatSettings`: when the env is
  set, build the local `ModelSettings` (provider `openai`, normalised base url),
  stash the prior `thinking` as `reasoning_fallback` (unless disabled), and swap
  `thinking` to the local model. Applied at all three `load_chat_settings`
  return points (settings.json / ANTHROPIC / default), so it wins regardless of
  where the base settings came from.
- `llm_call_with_fallback(primary, fallback, …, cache_key)`: run primary (via
  `llm_call_cached` when a cache_key is given, else `llm_call`); on `Err`, retry
  with `fallback` if present. The mcp fallback keeps its checksum cache; the local
  openai primary isn't long_context so it isn't cached.
- `reasoning_fallback_for(model, settings)`: returns `reasoning_fallback` only
  when `model` IS the overridden thinking slot — so planner/debug that resolve to
  `thinking` get the fallback too, but a coder/other slot never does.
- Wire the three thinking-slot consumers in the `/chat` turn: supervisor
  reasoning, planner, debug.

## Not in scope
- Coder slot stays on the tina4 coder (code emission + the 16K window).
- No dev-admin UI panel (env-only for now).

## Constraint (documented, not code)
Local `ctx-reader` caps at `max_model_len=16384` total; the reasoning prompt must
fit input+output. The mcp checksum cache does NOT apply to a plain OpenAI
endpoint, so the full prompt is sent each call. Big threads may need the fallback.

## Tests (real, no mocks — pure logic)
- `apply_local_reasoning_override`: env set → thinking becomes the local model
  (provider openai, normalised url, right model), fallback = prior thinking; env
  unset → unchanged; `/v1` trailing stripped; FALLBACK=0 → no fallback.
- `reasoning_fallback_for`: the thinking model → Some(fallback); a different model
  (coder) → None; no override → None.
- `cargo test` + `clippy` + release build.

## Status: ✅ Complete — 218 tests + 2 new (override + fallback) green; clippy clean; release builds
