# Thread 3 — New route serves without an app restart

## Goal
After the agent builds a resource, the new endpoint should serve **live** — no
manual migrate, no app restart.

## Findings (verified)
- The framework's `_auto_discover("src")` already imports **new** route modules;
  it runs on `POST /__dev/api/reload`. Confirmed: after the ping a new route goes
  404 → registered.
- Two things block "just works":
  1. The migration isn't applied → the new table is missing → route 500s.
  2. Nothing *triggers* reload when the agent writes files directly (the
     playground runs `python app.py`, no file watcher). `tina4 serve`'s watcher
     would, but auto-migrate still wouldn't happen.
- Verified the fix by hand: `tina4python migrate` + `POST /__dev/api/reload` →
  `GET /api/gadgets` returns **200**.

## Scope
- [ ] `run_migrate(project_dir)` helper (like run_framework_generate) — applies
      pending migrations via the framework CLI.
- [ ] After a scaffold build (which creates migrations/routes), the agent:
      run_migrate, then `POST http://127.0.0.1:{port-2000}/__dev/api/reload`
      (best-effort — the app may not be running).
- [ ] Thread the agent `port` into the coder handlers (framework port = port-2000).
- [ ] Wire into all three coder paths (code action, execute_plan, /execute).
- [ ] Report "migrated + reloaded" in the SSE so the user knows it's live.

## Tests (real — no mocks)
- [ ] Live app running; agent builds a resource → `GET /api/<res>` returns 200
      WITHOUT a manual migrate or restart.
- [ ] App NOT running → build still succeeds (reload ping fails silently).

## Bugs
- [ ] (none)

## Verification
- [x] Live app on 7170 + agent on 9170; `/execute` a gizmos plan →
      "✅ migrated + reloaded — live (no restart)"; `GET /api/gizmos` went
      **404 → 200** immediately, no app restart.
- [x] 95 Rust tests pass, clippy clean.

Note: the agent reaches the app at `PORT` = agent_port − 2000 (the framework
reads env `PORT`). Standard `tina4 serve` keeps them 2000 apart.

## Status: ✅ Complete — a freshly-built endpoint serves live.
