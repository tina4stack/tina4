# Thread 1 — Tests-first in the coder loop

## Goal
The independent agent builds runnable code but writes no tests. The skill is
emphatic ("No Code Without Tests, write the test FIRST"). Make the default agent
emit a real test alongside each resource it builds, so a build is *verified*, not
just *runnable*.

## Context
- The Python generators already **co-emit a real test** for a generated route
  (the CLI's `_gen_route(..., emit_test=True)`), so a scaffolded resource can
  ship with a test for free — we may just need to surface/run it.
- For custom (tina4_chat-authored) code there is no test yet.

## Scope
- [ ] Confirm what `tina4python generate route X --model Y` already emits for
      tests (path, shape) — verify against the live generator, don't assume.
- [ ] Ensure the scaffold path keeps/report the generated test file in
      `files_written` (so the user sees it).
- [ ] For a resource build, run the test after scaffolding (`tina4python test`
      or pytest on the emitted file) and report pass/fail in the SSE stream.
- [ ] For tina4_chat-authored custom code: emit a minimal real test (one
      request/response assertion) to the co-located `tests/` path.
- [ ] Surface a ✅/❌ per built artifact in the coder's final message.

## Tests (real — no mocks)
- [ ] Build a `products` resource in a temp `tina4python init` app → the emitted
      test file exists AND passes against a real SQLite DB.
- [ ] A build with a failing/edited handler → the test run reports ❌ (negative).

## Bugs
- [ ] (log here as [ ], tick with the commit when a real test proves it fixed)

## Commits
- (hash  description)

## Verification
Run in the playground / a temp init app; a build is only "done" when its test
runs green for real.

## Status: Proposed — scope check before building
