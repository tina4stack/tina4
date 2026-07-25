# Master Plan — Independent Coding Agent

Each row is its own small **context thread** with its own plan file. Pick one,
plan it, drive it to done, report, commit — then move to the next. The main
session stays free.

Branches: the bulk landed on `main`; the proof-only MCP work (Thread 11) is
isolated on `feature/supervisor-mcp` so `main` stays releasable until merged.

| # | Thread | Plan | Status |
|---|--------|------|--------|
| 0 | Independent no-Claude agent (topology, coder redesign, generate-first, essence) | [mcp-context-grounding.md](mcp-context-grounding.md) | ✅ Complete (runnable, direct + plan paths) |
| 1 | Tests-first in the coder loop | [tests-first-coder.md](tests-first-coder.md) | ✅ Complete — builds verified (tests emitted + run + ✅/❌) |
| 2 | Auth enforcement — is `POST` without token really open? | (resolved in Thread 1) | ✅ Resolved — writes ARE gated (401) |
| 3 | New-route discovery without an app restart | [live-route-discovery.md](live-route-discovery.md) | ✅ Complete — build auto-migrates + reloads; endpoint serves live |
| 4 | Supervisor routing: "go ahead" → execute_plan | _tbd_ | 🟡 Partly — ⚡ Build-it-now + /execute reliable; conversational path still meanders |
| 5 | Coders for nodejs/php/ruby (tina4_chat is Python-only) | _tbd_ | 🟡 Superseded — coder moved to `long_context` (Thread 9), language-framed per project |
| 6 | Dev-admin scaffold-endpoint parity (python/php/ruby/node) | [dev-admin-parity.md](dev-admin-parity.md) | ✅ Complete — all 4 verified live |
| 7 | Dev tools deploy dependencies (persist + dev-aware, all 4 langs) | [dev-tools-deploy-deps.md](dev-tools-deploy-deps.md) | ✅ Complete — verified live (uv add, init scaffolds pyproject) |
| 8 | NL→scaffold: extract fields (--fields) + stop phantom resources | [fix-nl-field-extraction.md](fix-nl-field-extraction.md) | ✅ Complete — live build lands name+price in schema, no phantom |
| 9 | Honest failures + long_context coder + the verification ladder | [honest-failures-and-long-context-coder.md](honest-failures-and-long-context-coder.md) | ✅ Complete — import/symbol/execution/write/render verify + rollback; reserved-word + reload fixes |
| 10 | Frontend generate-first (tina4-js pages/components) | [frontend-generate-first.md](frontend-generate-first.md) | ✅ Slices 1–2 done — agent builds a styled reactive page, rendered live |
| 11 | Proof-only MCP: remote AI builds locally, source never leaves | [mcp-proof-only-remote-build.md](mcp-proof-only-remote-build.md) | 🟡 Slice 0 done (validated w/ curl + local Ollama; backend+frontend build tools); tunnel/consent/registration TBD |

## Rules of the method
- One thread at a time; scope it in its plan file first, get a nod, then build.
- Tests first, real (no mocks), positive + negative — before the code.
- `[x]` only when it runs green for real; log the commit hash in the thread's plan.
- New work = new checkboxes in a plan, never an off-plan side-quest.

## Playground for trying threads live
`tina4-dev-admin/.playground` (fresh `tina4python init`) + the new agent on 9150
+ dev-admin on 5173. Build via the Threads pane, watch it run on 7150.

Ports: agent = framework + 2000 (the `tina4 serve` convention). The playground
moved off 7146/9146 — another local project holds 7146, and the SPA silently
proxied to THAT app instead (wrong file tree, wrong routes, confusing results).
Check `lsof -nP -iTCP:<port> -sTCP:LISTEN` before assuming a port is yours.
