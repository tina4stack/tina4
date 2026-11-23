# Task: #160 skills refresh fails when the whole install takes over 60 seconds

Issue: https://github.com/tina4stack/tina4/issues/160 (CLI 3.8.115, skills ref 3.13.165).
Branch: `fix/skills-fetch-timeout-160` off `main`. PR-only.

Outcome: a slow install that is still making progress completes; a host that
stops answering is still caught, per file, instead of by a 60 s budget on the
whole run; the failure hint names a command that works from any directory.

## Cause (verified in src/skills.rs at 1e01e74)
- `FETCH_BUDGET_SECS = 60` is checked against one `started` instant set once in
  `install_inner`, so it limits TOTAL install time, not a single download.
- Files (99 per target at 3.13.165) are fetched one after another, one `curl`
  spawn each, so total time grows with the manifest and crosses 60 s on an
  ordinary connection.
- The budget exists only because `download_with_curl` passes no timeout.
- The hint says `tina4 ai`, which needs a project; `tina4 skills <target>` does not.
- `fetch_latest_ref` also downloads with no timeout (a mute tina4.com hangs
  before the install even starts).

## Scope
- [x] `download_file_bounded(url, dest, stall)` in main.rs: curl gets
      `--connect-timeout` + `--speed-limit 1 --speed-time <stall>` (no-progress
      timeout) + a per-file `--max-time`; wget `-T`; PowerShell `-TimeoutSec`.
      The self-update path keeps the unbounded primitive (a large binary on a
      slow link must not be cut off).
- [x] skills.rs: drop `FETCH_BUDGET_SECS` / `started`; every fetch uses the
      bounded primitive with `Config.stall_timeout` (default 30 s,
      `TINA4_SKILLS_STALL_TIMEOUT` override).
- [x] `fetch_latest_ref` uses the bounded primitive.
- [x] Fetch the manifest's files in parallel (small fixed worker pool, shared
      dead-host list); first failure stops the run; nothing is published unless
      every file landed and verified (unchanged).
- [x] Hint: `tina4 skills <target>`.

## Tests (written first, real - real binary, real local HTTP server, no mocks)
- [x] slow-but-progressing install completes although its total time exceeds
      the stall timeout (fails against a whole-run budget)
- [x] a host that accepts and never answers fails the install within a bounded
      time and names the failure (hangs against the unbounded download)
- [x] the failure hint names `tina4 skills <target>`, not `tina4 ai`

## Bugs
- [x] #160 whole-run budget aborts healthy slow installs
- [x] #160 recovery hint fails outside a project

## Not in scope (noted)
- install-skills.sh / .ps1 also fetch with no timeout; changing them needs a
  re-sign of the .ps1 and a skills release. Separate change.
- `refresh_installed_skills` downloads the full set once per target (claude +
  codex = twice). Parallel fetch makes it cheap; a shared download is a refactor.

## Commits

## Status: In Progress
