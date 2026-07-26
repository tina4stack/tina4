# Thread 4 — Conversational routing: enforce sign-off in CODE, not just the prompt

## The problem (one falsifiable claim)
> After the planner emits a plan, a clear user sign-off ("go", "ok", "yes",
> "looks good", "do it") sometimes yields `{"action":"respond"}` with narration
> ("I'll set that up…") instead of executing the plan. The build stalls and the
> user has to repeat themselves.

## Root cause (verified in `src/agent.rs`)
Every sign-off / "go-phrase" rule lives in the **system prompt** (lines ~468–558:
the go-phrase list, "ACT IMMEDIATELY", "return execute_plan IMMEDIATELY"). There
is **no code-side detection**. Routing is 100% "trust the model to comply." The
supervisor now runs on `long_context` (weaker than Claude), which narrates
instead of acting — so the prompt guidance is not reliably followed. This is the
same failure class the codebase already fixes elsewhere by **inferring intent in
code** rather than trusting compliance (Thread 8 `## APPEND:` coercion; the
existing `execute_plan` newest-plan fallback).

The dispatch site: `parse_supervisor_action` (line 3633) → `match action` (3647).
Nothing corrects a `respond`/`UNPARSED` result when the user plainly signed off.

## The fix — a deterministic sign-off guard (additive, ~1 insertion + 2 helpers)
Right after the parse + log (after line 3645), coerce the action when intent is
unambiguous:

```
action = coerce_signoff_to_execute(action, &chat_req.message, &recent, latest_plan.is_some());
```

- `is_signoff(msg)` — normalise (lowercase, trim, strip trailing punctuation);
  return true only for a short affirmative ("go", "go ahead", "ok", "yes", "do
  it", "build it", "ship it", "proceed", "looks good", "lgtm", "make it
  happen", …). The phrase list is lifted from the prompt into a shared `const`
  so prompt and code agree. **Revision cues veto it** ("but", "change",
  "instead", "actually", "wait", "no ", "add ", "remove", "also", "except") —
  "yes but change the colour" is a revision, not a sign-off.
- `plan_awaiting_signoff(recent)` — true when the **last assistant turn** came
  from the planner (`agent == "planner"`) or reads as a plan (numbered list ≥3
  steps). This is the gate that makes matching bare "yes"/"ok"/"go" safe: we only
  coerce when a plan is actually waiting.
- `coerce_signoff_to_execute(...)` — if `is_signoff` AND the model did **not**
  already act (`execute_plan`/`plan`/`code`) AND `plan_awaiting_signoff` AND a
  plan file exists → return `execute_plan` with `context:"plan/"` (the existing
  newest-plan fallback resolves the file). Otherwise return the action unchanged.
  Log `supervisor.signoff_coerce` when it fires, so overrides are visible in
  agent.log (mirrors the existing `execute_plan.fallback` logging).

### Deliberately NOT in scope (keep it tight, low-risk)
- No `respond → plan` coercion (no plan yet = genuinely needs requirements; too
  easy to misfire on a bare "ok"). Only the **post-plan** case, which is the
  documented failure.
- No prompt rewrite. The prompt stays; the guard is a safety net under it.

## Tests (real, positive + negative — no mocks)
Unit tests on the pure helpers:
- `is_signoff`: ✅ "go", "go ahead", "ok", "yes", "do it", "LGTM", "ship it 🚀";
  ❌ "yes but change the price", "actually add email", "no", "what about auth?",
  "can you also…".
- `plan_awaiting_signoff`: ✅ last turn agent=="planner" / numbered list;
  ❌ last turn a plain supervisor question, empty history.
- `coerce_signoff_to_execute`: ✅ ("go" + plan pending + plan file) → execute_plan;
  ❌ ("go" + no plan pending) unchanged; ❌ ("go" but action already execute_plan)
  unchanged; ❌ ("yes but…" + plan pending) unchanged.
- Full `cargo test` + `cargo clippy` green before claiming done.

## Live verification
Drive the playground (agent 9150 / app 7150): build request → planner plan →
reply "go" (and separately "yes, looks good") → the build executes without
re-asking; agent.log shows `supervisor.signoff_coerce`. Then a revision ("yes but
rename it") → NOT coerced, routed as a revision.

## Status: 🟢 Built + unit-verified; live drive pending
- Guard implemented in `src/agent.rs`: `SIGNOFF_PHRASES`/`REVISION_CUES` consts,
  `normalise_signoff`, `is_signoff`, `plan_awaiting_signoff`,
  `coerce_signoff_to_execute`, wired at the `/chat` dispatch after
  `parse_supervisor_action`, logging `supervisor.signoff_coerce` when it fires.
- 6 real unit tests (positive + negative) green; full suite 160 passed / 0
  failed; `cargo clippy` clean for `agent.rs` (only pre-existing warnings in
  `session.rs`/`session_smoke.rs`); `cargo build --release` OK.
- LIVE DRIVE still to do: rebuild + restart the playground agent (9150) and
  confirm "go" after a plan executes (log shows `supervisor.signoff_coerce`) and
  "yes but rename it" does not. Deferred because it restarts the running
  playground and triggers a real build — do with the user's go-ahead.

```
Branch: thread4/conversational-signoff-guard (off main)
Commit hash: (this commit)
```
