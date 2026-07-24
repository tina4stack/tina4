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

## Open findings (surfaced by the honest reporting — NOT yet fixed)
- [ ] **`order` is a SQL reserved word.** The generator emits
      `CREATE TABLE IF NOT EXISTS order (...)` unquoted → syntax error, table
      never created, endpoint 404. Needs identifier quoting (or pluralised
      table names) in tina4-python's generator. Fields themselves were correct
      (`total REAL, status VARCHAR(255)`).
- [ ] **`long_context` invents route-param file paths** — it tried to write
      `src/routes/orders/{id}.py` instead of using tina4's `@get("/api/orders/{id}")`
      decorator convention. The path guard correctly refused it. Needs coder
      prompt/grounding guidance on tina4 routing.

## Status: ✅ Complete for the reporting + coder switch; two findings logged above.
