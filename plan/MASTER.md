# Master Plan — Independent Coding Agent (feature/independent-coding-agent)

Each row is its own small **context thread** with its own plan file. Pick one,
plan it, drive it to done, report, commit — then move to the next. The main
session stays free.

| # | Thread | Plan | Status |
|---|--------|------|--------|
| 0 | Independent no-Claude agent (topology, coder redesign, generate-first, essence) | [mcp-context-grounding.md](mcp-context-grounding.md) | ✅ Complete (runnable, direct + plan paths) |
| 1 | Tests-first in the coder loop | [tests-first-coder.md](tests-first-coder.md) | ✅ Complete — builds verified (tests emitted + run + ✅/❌) |
| 2 | Auth enforcement — is `POST` without token really open? | (resolved in Thread 1) | ✅ Resolved — writes ARE gated (401); earlier 201 was a bare `.env` with no `TINA4_SECRET` |
| 3 | New-route discovery without an app restart | [live-route-discovery.md](live-route-discovery.md) | ✅ Complete — build auto-migrates + reloads; endpoint serves live |
| 4 | Supervisor routing: "go ahead" → execute_plan | _tbd_ | 🟡 Next |
| 5 | Coders for nodejs/php/ruby (tina4_chat is Python-only) | _tbd_ | ❌ Not started |
| 6 | Dev-admin scaffold-endpoint parity (python/php/ruby) | _tbd_ (dev-admin repo) | ❌ Not started |
| 7 | Dev tools deploy dependencies (persist + dev-aware, all 4 langs) | [dev-tools-deploy-deps.md](dev-tools-deploy-deps.md) | ✅ Complete — verified live (uv add, init scaffolds pyproject) |

## Rules of the method
- One thread at a time; scope it in its plan file first, get a nod, then build.
- Tests first, real (no mocks), positive + negative — before the code.
- `[x]` only when it runs green for real; log the commit hash in the thread's plan.
- New work = new checkboxes in a plan, never an off-plan side-quest.

## Playground for trying threads live
`tina4-dev-admin/.playground` (fresh `tina4python init`) + the new agent on 9146
+ dev-admin on 5173. Build via the Threads pane, watch it run on 7146.
