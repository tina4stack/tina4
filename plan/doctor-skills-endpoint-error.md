# Task: doctor skills-currency distinguishes an HTTP error (404) from offline

**Outcome:** `tina4 doctor` no longer hides a broken skills-version-check endpoint
behind a benign "offline" line. A curl HTTP error (exit 22, i.e. the endpoint
answered 4xx/5xx — moved/renamed/misconfigured) is surfaced as a distinct yellow
warning; a genuine transport failure (DNS/connect/timeout) stays the blue
"offline" info it is today.

## Scope
- [x] Reproduce/confirm the defect in `tina4/src/doctor.rs`
- [x] `enum LatestRef { Found(String), HttpError, Offline }` + pure `latest_from_curl(...)` (IO-free, testable)
- [x] `fetch_latest_skills_ref()` returns `LatestRef` (branch on curl exit code; keep `-f`)
- [x] `SkillsStatus::CheckEndpointBroken(Option<String>)` variant + `classify_skills` consumes `LatestRef`
- [x] Add the `CheckEndpointBroken` arm to ALL THREE print blocks (Claude / Codex / Cursor)
- [x] Tests (pure, mutation-proved) — see below
- [x] `cargo test` green (321 passed, 0 failed)

## Parity
CLI-only feature (the skills-currency check exists only in the Rust CLI). No
Python/PHP/Ruby/Node port. The three IN-CLI consumers (Claude, Codex, Cursor)
must stay identical — the exhaustive `match` compiler-enforces it.

| Surface | Status |
|---------|--------|
| Claude (~/.claude/skills) | ✅ |
| Codex (~/.agents/skills)  | ✅ |
| Cursor (~/.cursor/skills) | ✅ |

## Root cause (confirmed)
`fetch_latest_skills_ref` runs `curl -fsSL` and returns `None` on any non-zero
exit (`doctor.rs:426`). `-f` collapses a 404 and a real network failure into the
same `None`, so `classify_skills(.., None)` -> `InstalledUnknownLatest` ->
"could not check latest (offline)" (`doctor.rs:479`), a dimmed blue INFO line.
A permanently-broken endpoint therefore reads as a transient blip and no
staleness/actionable marker ever shows. curl already distinguishes the two via
its exit code: **22 = HTTP >= 400** (reachable, broken) vs 6/7/28/... (offline).

## Tests (written first, real — no mocks, positive + negative)
Pure-logic unit tests (no dependency, no double — the only no-live-dep case):
- [ ] `latest_from_curl(true, Some(0), Some("1.2.3"))` -> `Found("1.2.3")`
- [ ] `latest_from_curl(true, Some(0), None)` -> `Offline`  (200 but unparseable = benign)
- [ ] `latest_from_curl(false, Some(22), None)` -> `HttpError`   ← the 404 case
- [ ] `latest_from_curl(false, Some(7), None)` -> `Offline`      ← genuine offline
- [ ] `latest_from_curl(false, None, None)` -> `Offline`         ← curl spawn failed
- [ ] `classify_skills(true, Some("1.0.0"), HttpError)` -> `CheckEndpointBroken(Some("1.0.0"))`
- [ ] `classify_skills(true, None, HttpError)` -> `CheckEndpointBroken(None)`
- [ ] `classify_skills(true, Some("1.0.0"), Offline)` -> `InstalledUnknownLatest("1.0.0")` (unchanged)
- [ ] existing Current/Stale/Unknown tests updated to the `LatestRef` signature
- [ ] MUTATION WITNESS: flip `Some(22) => HttpError` to `_ => Offline` and the HttpError
      test + the CheckEndpointBroken test both go red.

## Bugs
- [x] doctor 404 masked as offline (fixed; mutation-proved via `latest_http_error_on_curl_exit_22`)
- [x] (found in-pass) CLAUDE.md version header drifted 3.8.76 vs Cargo.toml 3.8.77 — aligned to 3.8.77

## Commits
- b07fd52  fix(doctor): distinguish a broken skills-check endpoint from offline; align CLAUDE.md version 3.8.76->3.8.77

## Status: Complete
