# Master Plan — Independent Coding Agent

Each row is its own small **context thread** with its own plan file. Pick one,
plan it, drive it to done, report, commit — then move to the next. The main
session stays free.

Branches: everything is on `main` and pushed. Recent session merged five PRs —
long_context checksum caching (#11), local reasoning-slot override + mcp fallback
(#12), trailing-text parse tolerance + planner/debug on the strong model (#13),
plan-driven scaffold-first + resource-naming fix (#14), and init-defaults-to-SQLite
(#15). `main` carries all threads.

| # | Thread | Plan | Status |
|---|--------|------|--------|
| 0 | Independent no-Claude agent (topology, coder redesign, generate-first, essence) | [mcp-context-grounding.md](mcp-context-grounding.md) | ✅ Complete (runnable, direct + plan paths) |
| 1 | Tests-first in the coder loop | [tests-first-coder.md](tests-first-coder.md) | ✅ Complete — builds verified (tests emitted + run + ✅/❌) |
| 2 | Auth enforcement — is `POST` without token really open? | (resolved in Thread 1) | ✅ Resolved — writes ARE gated (401) |
| 3 | New-route discovery without an app restart | [live-route-discovery.md](live-route-discovery.md) | ✅ Complete — build auto-migrates + reloads; endpoint serves live |
| 4 | Supervisor routing: "go ahead" → execute_plan | [conversational-routing-signoff-guard.md](conversational-routing-signoff-guard.md) | ✅ Sign-off enforced in code (deterministic guard, 6 tests, green + merged); live playground drive optional |
| 5 | Coders for nodejs/php/ruby (tina4_chat is Python-only) | _tbd_ | 🟡 Superseded — coder moved to `long_context` (Thread 9), language-framed per project |
| 6 | Dev-admin scaffold-endpoint parity (python/php/ruby/node) | [dev-admin-parity.md](dev-admin-parity.md) | ✅ Complete — all 4 verified live |
| 7 | Dev tools deploy dependencies (persist + dev-aware, all 4 langs) | [dev-tools-deploy-deps.md](dev-tools-deploy-deps.md) | ✅ Complete — verified live (uv add, init scaffolds pyproject) |
| 8 | NL→scaffold: fields + scaffold-first for plan-driven builds | [fix-nl-field-extraction.md](fix-nl-field-extraction.md) | ✅ Complete + extended — direct builds land fields; plan-driven `/execute` now scaffolds the resource ONCE from the GOAL and skips covered prose steps (#14); resource-naming ignores leading verbs / DB names; fresh `tina4 init` binds SQLite (#15). Verified live: `GET /api/widgets → 200` |
| 9 | Honest failures + long_context coder + the verification ladder | [honest-failures-and-long-context-coder.md](honest-failures-and-long-context-coder.md) | ✅ Complete — import/symbol/execution/write/render verify + rollback; reserved-word + reload fixes |
| 10 | Frontend generate-first (tina4-js pages/components) | [frontend-generate-first.md](frontend-generate-first.md) | ✅ Slices 1–2 done — agent builds a styled reactive page, rendered live |
| 11 | Proof-only MCP: remote AI builds locally, source never leaves | [mcp-proof-only-remote-build.md](mcp-proof-only-remote-build.md) | 🟡 Slice 0 done + merged to `main` (validated w/ curl + local Ollama; backend+frontend build tools); tunnel/consent/registration TBD |
| 12 | Reason on a local/hosted model (checksum, override, fallback) | [reasoning-slot-override.md](reasoning-slot-override.md), [long-context-checksum.md](long-context-checksum.md) | ✅ Complete — `TINA4_LOCAL_MODEL_*` points the reasoning slot at a local/hosted model (chat.tina4.com `general`), fallback to mcp `long_context` (#12); planner/debug stay on the strong model (#13); `long_context` checksum caching appends deltas not the whole corpus (#11); trailing-text parse tolerance (#13). Verified live in the dev-admin |

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
