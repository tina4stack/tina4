# Task: FREE-TOKEN trial mode — try the dev-admin coder before signup

## Scope
mcp.tina4.com has NO anonymous mode (verified: every call without a valid Bearer
token → 401 "register at profile.tina4.com"). The coder/reasoning path also
hard-fails without a token (agent.rs long_context/tina4_chat "unavailable"). So
"try before signup" requires a REAL shared credential, not a client trick.

Decision: a shared **FREE-TOKEN** (literal value `FREE-TOKEN`) that the supervisor
sends by default when the developer has set no personal `TINA4_MCP_TOKEN`. Andre
activates `FREE-TOKEN` as a **rate-limited** trial credential on tina4.com's auth
(mcp.tina4.com, and chat.tina4.com/general if the hosted reasoning model is also
to be free). Client sends it; server caps it.

- [x] `mcp_context::token()` gains a 3rd rung: personal env → personal `.env` → FREE-TOKEN
- [x] `FREE_TOKEN` constant (`"FREE-TOKEN"`), overridable via `TINA4_FREE_TOKEN` env
- [x] `TokenSource` (Personal | Free | None) + `token_source()` for status/nudge
- [x] `has_personal_token()` so the UI distinguishes "your token" vs "free trial"
- [x] `/mcp/status` reports `source` ("personal" | "free") — wire-verified
- [ ] throttled in-chat signup nudge when running on the free token — deferred (panel banner covers it; add if the banner proves insufficient)
- [x] dev-admin grounding panel: persistent "🎁 Free trial — register" banner on free
- [ ] PARITY: `source` in the 3 self-contained snapshots (Python/PHP/Ruby) — display-only,
      so the banner shows on those hosts too (Node proxies the agent, already covered). Pending nod.
- [x] SERVER (Andre): activate literal `FREE-TOKEN` as a rate-limited credential on mcp.tina4.com
      (+ chat.tina4.com/general for the hosted reasoning model). Client sends it; server caps it.

## Parity
Supervisor is the Rust CLI (single impl), dev-admin is the single SPA. No 4-language
port — this is agent tooling, not framework runtime. (Grounding token handling already
lives only in the Rust supervisor.)

## Tests (written first, real — no mocks, positive + negative)
- [x] resolve() prefers personal over free                                 (pure, mutation-proven)
- [x] resolve() falls back to FREE-TOKEN when no personal                   (pure, mutation-proven)
- [x] resolve() blank personal ignored → free                              (pure)
- [x] resolve() none when free disabled AND no personal                     (pure)
- [x] free_token_from(None) = literal constant; env override wins; blank disables (pure)
- [x] personal_token() reads .env, counts as Personal                       (real tempdir)
- [x] has_personal_token() false on a bare project                          (real tempdir)
- [x] frontend: groundingStatusView free/personal/none + 2 back-compat cases (happy-dom, mutation-proven)
- [x] WIRE: agent /mcp/status → source:"free" bare, source:"personal" with .env token (real boot+curl)

## Bugs
- (none)

## Commits
- (dev-admin)  FREE-TOKEN trial banner + groundingStatusView + real tests
- (tina4)      FREE-TOKEN 3rd-rung resolver + TokenSource + /mcp/status source

## Status: Client and hosted FREE-TOKEN complete + wire-proven. CLI release pending;
  display-only `source` parity for Python/PHP/Ruby snapshots remains pending.
