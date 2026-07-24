# Thread 9 — Honest failure reporting + coder on long_context

## Why
A live build reported **"All done ✓"** having written nothing. Root cause chain,
found by end-to-end testing:

1. `tina4_chat` answers HTTP **200 with prose** ("currently offline or under
   maintenance") instead of code.
2. The agent parsed that prose as coder output, refused it as a bogus path,
   logged `step.skipped` — **then still marked the step ✓ and recorded it
   completed**, so `--resume` would skip real work too.

## Why tina4_chat said "maintenance" (measured, not guessed)
It is **prompt size**, not downtime or a restart:

| prompt | result |
|--------|--------|
| ~0.3 / 3.2 / 5.0 / 6.9 / 8.7 KB | CODE OK |
| ~10.5 / 12.4 / 30.7 KB | **MAINTENANCE** |

Threshold ≈ **9 KB** (~2.5k tokens). The agent's grounded prompt (framework +
project context + full plan + step) routinely exceeded it, so real builds always
got the notice while small probes always worked.

## Scope
- [x] `coder_unavailable_notice()` — treat a 200-with-availability-prose as an
      OUTAGE: fail the step, emit `plan_failed`, never write it to disk.
- [x] A step where **every** write was refused → `✗`, NOT recorded completed
      (a step that legitimately writes no files, e.g. "run the tests", still passes).
- [x] Both plan paths (`/execute` + `execute_plan`) fixed.
- [x] **Coder switched to `long_context`** (user's call) — large window, so the
      whole class of size-driven failures disappears.
- [x] Generate-first **decoupled from `tina4_chat`** — scaffolding is the textbook
      path for every coder; MCP coders skip the citation-verify retry (that gate
      is for Claude).
- [x] `derive_coder_path` honours an explicit path in the step
      ("…in src/app/helpers.py").
- [x] `## FILE:` header synthesised for ANY coder returning a bare code fence.
- [x] Red suite can never report ✅ — `summary_reports_failure()` (the framework
      CLI's exit code alone is not trusted).

## Verification (real)
- [x] 123 Rust tests, clippy clean.
- [x] Stub MCP returning the exact outage string → step reports
      `✗ (coder unavailable)`, `plan_failed` emitted, `state.json` =
      `{"completed": [], "files": []}`, **no prose written to disk**.
- [x] With `long_context`: the same previously-failing plan **completed** —
      `src/app/helpers.py` written with real code + `# grounded-by: [0]`.
- [x] Generate-first still fires under the new coder (4 + 2 files scaffolded).
- [x] `tina4python test` exit code: installed tool was stale (exited 0 on a red
      suite); reinstalled → now exits 1. Agent no longer trusts it either.

## Open findings (surfaced by the honest reporting)
- [x] **`order` is a SQL reserved word** — FIXED in tina4-python `9563815`.
      `_to_table()` now pluralises a reserved name (Order → orders); every
      generator routes through it so model/migration/routes/tests agree.
      Verified: table creates, suite 15 passed (was 4 failed),
      `GET /api/orders` → `{"id":1,"total":99.5,"status":"paid"}`.
      Note the ORM still interpolates table names UNQUOTED and passes the raw
      name to driver insert/update/delete — a hand-set
      `table_name = "order"` would still break. Proper identifier quoting in
      the ORM/drivers remains open (bigger, dialect-aware change).
- [x] **Routing/file conventions** — FIXED (`774dfa7`). `TINA4_CODER_CONTRACT`
      + guards. Both models now emit `src/routes/orders.py` with
      `@get("/api/orders/{id}")`. (long_context had been writing FastAPI;
      tina4_chat had been writing into `python/tina4_python/cli/__init__.py`.)
- [x] **Edits rewrote whole files and lost code** — FIXED (`2a73ffe`).
      Patch-based `## APPEND:`, concatenation done by the agent, plus intent
      inference when the model ignores the instruction. Verified: 5 → 6
      handlers, none lost.
- [x] **Invented ORM methods** — FIXED (`7da4f41`). Verified against the
      installed framework + one corrective retry.

## Still open
- [ ] **Misuse of a REAL method isn't caught.** Symbol verification proves a
      method exists, not that it's called correctly. Live: the retry produced
      `Order.select("SUM(total) as total_revenue").first()` — `select` is real
      but takes a COMPLETE SQL query (`near "SUM": syntax error`), and `.first()`
      is not a list method. Static checking can't close this; the fix is
      EXECUTION — require a co-emitted test for an appended handler, or smoke
      the new endpoint after reload and fail the step on a 500.
- [ ] **Reload does not re-import EDITED modules.** `POST /__dev/api/reload`
      discovers NEW route files but an already-imported module stays cached in
      `sys.modules`, so an edited handler keeps serving the OLD code (observed:
      the endpoint kept raising `'Order' has no attribute 'sum'` after the file
      no longer said `sum`). "live (no restart)" is therefore only true for NEW
      files. Framework fix: drop/reload changed modules on reload.
- [ ] ORM identifier quoting, so a hand-set `table_name = "order"` works.

## Status: ✅ Complete for the reporting + coder switch; two findings logged above.
