# Task: Release and sign Tina4 CLI v3.8.73

**Outcome:** Publish a signed Tina4 CLI with cross-provider thinking mode streaming (OpenAI/Anthropic/MCP), reasoning deltas in supervisor/planner/debug agents, and robust null-content fallback.

## Scope
- [x] Stream thinking tokens & content deltas across OpenAI (`openai_call_stream`) and Anthropic (`anthropic_call_stream`) endpoints
- [x] Wire SSE streaming for Supervisor, Planner, and Debug agents
- [x] Support reasoning-content fallback and robust null-content parsing for reasoning models (Bonsai, DeepSeek, Qwen)
- [x] Add unit tests for OpenAI and Anthropic SSE delta parsing and reasoning content
- [x] Bump CLI version to 3.8.73 (Cargo.toml + CLAUDE.md)
- [x] Run full Rust test suite (`cargo test`)
- [x] Commit changes, tag `v3.8.73`, and push to trigger CI
- [x] GitHub Actions workflow completed: published crate `tina4` 3.8.73, CLI container image, and draft release assets for all 7 platforms
- [ ] Sign Windows binary on macOS (`sign-mac.sh v3.8.73`) and publish release (awaits SimplySign Desktop 2FA session)

## Tests (real, positive + negative)
- [x] `parse_openai_sse_line_splits_thinking_and_content` (DeepSeek reasoning_content, thinking, content, done, comments)
- [x] `parse_anthropic_sse_line_splits_thinking_and_text` (thinking_delta, text_delta, message_stop)
- [x] `llm_response_parses_null_content_with_reasoning` (reasoning_content when content is null)
- [x] Full test suite: 308 executed passed
