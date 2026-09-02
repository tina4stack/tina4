# Skills communication-style eval

A skill is instructions, not code. You cannot unit-test a wording change. You
validate it by running the agent and scoring what it writes. This harness does
that for the Tina4 skills' **Communication Style** section.

## What it measures

Does the candidate wording make replies clearer for a global team of engineers?
Five things, each scored 0 or 1 per reply (see `rubric.md`):

1. Result-first
2. Concise
3. Plain language (readable by a non-native English speaker on the first read)
4. Right-sized (no over-thinking, no over-building)
5. Question-first only when blocked

## How to run

1. Take the task set in `tasks.md`.
2. Give one agent `arms/baseline.md` as its only style rule; give another
   `arms/candidate.md`. Each answers all tasks as it would in chat.
3. Run each arm at least 3 times (model output varies).
4. Score every reply with `rubric.md`.
5. Report the per-arm average and the per-dimension breakdown. Ship the
   candidate only if it wins with no dimension going backwards.

The agent under test must follow ONLY the arm block — tell it to ignore any
other tone or skill guidance, so the wording is the one thing that changes.

## Layout

```
README.md         this file
rubric.md         the five dimensions and how to score each
tasks.md          the task set (and the good answer for each)
arms/baseline.md  current Communication Style wording
arms/candidate.md proposed wording
results/          one dated file per run: outputs, scores, verdict
```

## Why these tasks

Each task stresses one or two dimensions on purpose — a trivial question (does it
stay short?), an ambiguous ask (does it ask instead of guess?), a "add one field"
task (does it build only what was asked?). See the notes in `tasks.md`.
