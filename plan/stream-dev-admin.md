# Task: Stream Bonsai thinking + tokens into tina4dev

**Outcome:** The MCP `long_context` tool stays a complete string (Cursor / JSON-RPC). Tina4dev consumes a stream: thinking deltas then answer tokens, then the assembled reply is parsed as the supervisor action.

- [x] MCP `POST /long_context/stream` proxies ctx-qa SSE (same auth as `/mcp`)
- [x] CLI `long_context_call_stream` + supervisor emits `thinking` / `token` SSE
- [x] tina4-dev-admin renders those events live
- [x] Tests for SSE frame parse (real fixtures, no mocks)

Commits: tina4 CLI + tina4-dev-admin local; MCP image after aatos push.
