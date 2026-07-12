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

## Findings (scope check — verified against the live generator)
- [x] The **generators already do tests-first**: `generate model X` emits
      `tests/test_<table>_model.py` (real SQLite roundtrip); `generate route x
      --model Y` emits `tests/test_x.py` — a real `TestClient` secure-gate test
      (GET public 200, tokenless POST 401, tokened POST 201). No mocks.
- [x] Ran the emitted route test: **3 passed** — auth IS enforced (this also
      resolves Thread 2; the earlier raw-HTTP 201 was a bare `.env` with no
      `TINA4_SECRET`, not a gap).
- [x] ROOT CAUSE the agent got no tests: the **installed `~/.local/bin/tina4python`
      is STALE** (source is 3.13.71 and emits tests; the installed CLI predates
      that). Calling the source `_gen_route`/`_gen_model` directly emits tests.

## Scope (revised)
- [x] Update the local `tina4python` to current source so `generate` emits its
      tests: `uv tool install --force --with pytest .` (also gives the tool env
      pytest so `tina4python test` is self-sufficient). CLI now writes
      `tests/test_*.py` and `tina4python test` runs them green.
- [x] Agent: emitted `tests/test_*.py` land in `files_written` automatically
      (run_framework_generate parses "Created …").
- [x] Agent: after a scaffold build, RUN the tests via `run_project_tests()`
      (`tina4python test`) and stream a ✅/❌ summary — wired in all THREE coder
      paths (code action, execute_plan action, /execute endpoint).
- [ ] tina4_chat-authored custom code: emit + run a minimal real test. (deferred
      — scaffolded resources are the common case and now covered.)

## Tests (real — no mocks)
- [x] `generate route ... --model` on the updated CLI writes `tests/test_*.py`;
      `tina4python test` → 5 passed (2 model roundtrip + 3 route auth-gate).
- [x] Agent /execute build of a widgets resource → "✅ Tests: 5 passed".

## Bugs
- [x] Installed `tina4python` stale — no test emission. Fixed: reinstalled from
      source (`uv tool install --force --with pytest .`).
- [x] Detection over-fire: "a model for widgets with a name and a price"
      generated a phantom `Price` model (rfind grabbed the trailing field). Fixed
      in `detect_resource_name` — cut " with "/" having "/" that has" field
      clauses before finding the resource noun. Re-tested: Widget only.

## Notes
- FRAMEWORK: `tina4python test` needs pytest available; a bare `init` project
  doesn't have it. We added it to the tool env locally. Consider whether the
  published tina4-python should ensure a test runner for `tina4python test`.
- Thread 2 (auth) is RESOLVED by this: the generated route test asserts tokenless
  POST → 401 and passes → writes are gated. The earlier raw-HTTP 201 was a bare
  `.env` with no `TINA4_SECRET`, not a framework gap.

## Status: ✅ Complete — scaffolded builds are verified (tests emitted + run).
