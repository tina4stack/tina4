# Task: Self-describing CLI — the client discovers commands from the framework

## Goal
Make the `tina4` Rust client **capability-driven**: it asks each framework CLI
what commands it supports (a machine-readable manifest) and forwards them,
instead of hardcoding the token contract. The client becomes agnostic — a new
framework command is picked up automatically; a missing one is simply not
offered. Kills the client<->framework drift class permanently.

## Context
A full client audit found the client hardcodes which tokens it delegates, and
those tokens have drifted out of parity with what the frameworks actually accept:

| Client sends | Python | PHP | Ruby | Node |
|---|:--:|:--:|:--:|:--:|
| migrate / :rollback / :status | OK | OK | OK | OK |
| migrate:create | OK | OK | MISSING | OK |
| seed / test / routes / metrics / ai / console | OK | OK | OK | OK |
| build | pkg-build (wrong target) | OK | MISSING | MISSING |
| queue work/stats/retry/clear | MISSING | OK | MISSING | MISSING |
| generate | client runs Rust STUBS, does not delegate | OK | OK | OK |

Root cause: the client is a second source of truth for the command surface.
Owner direction: the client should query the framework for its "methods" so it
is completely agnostic (2026-07-10).

## Design

### Framework side — a cheap capability manifest (Python master, mirror 3)
Add `<cli> commands --json` to each framework CLI. It prints the command table
the CLI already dispatches from — **without booting the app / DB / migrations**
(must be fast; it runs on every client help/refresh). Identical JSON shape in
all four:

```json
{
  "framework": "python",
  "version": "3.13.65",
  "commands": [
    { "name": "migrate",        "summary": "Run pending migrations" },
    { "name": "migrate:create", "summary": "Create a migration",  "args": ["description"] },
    { "name": "seed",           "summary": "Run seeders",         "args": ["name?"] },
    { "name": "queue",          "summary": "Queue worker",        "subcommands": ["work","stats","retry","clear"] },
    { "name": "generate",       "summary": "Scaffold",            "subcommands": ["model","route","crud","migration","middleware","test","form","view","auth","service","queue","validator","seeder","websocket","listener"] }
  ]
}
```

### Client side — merge native + discovered, forward the rest
- **Native (conductor) commands stay in Rust** — `serve`, `scss`, watch/reload,
  `setup`, `init`, `deploy`, `agent`, `doctor`, `install`, `update`, `books`,
  `docs`, `upgrade`. SCSS compilation stays client-side (owner: kept
  deliberately).
- **Everything else is discovered:** detect language -> run `<cli> commands
  --json` -> merge those into the client's help + dispatch table.
- **Dispatch is pass-through:** `tina4 <cmd> <args...>` -> `<framework-cli>
  <cmd> <args...>` verbatim. The framework owns arg parsing; the client no
  longer needs per-command flag knowledge (drops the `--create`->`migrate:create`
  translation — the manifest advertises `migrate:create` directly).
- **Dispatch needs no manifest:** the native (conductor) set is client-owned;
  ANY non-native command is forwarded blind and the framework rejects unknowns.
  So normal commands pay zero manifest cost. The manifest is consumed ONLY to
  render an accurate `tina4 --help` / command listing.
- **Cache** the manifest in `.tina4/commands.json` (gitignored), validated by a
  cheap `stat` **fingerprint** of the resolved framework CLI (path + mtime +
  size) — unchanged fingerprint => reuse without spawning; changed (upgrade OR
  local edit) => re-query + rewrite. Uniform across all 4 languages, catches
  editable installs a version-check would miss. Plus a `--refresh` escape hatch.
- **Graceful fallback:** cache miss + spawn fails, or an older framework CLI with
  no `commands` subcommand -> the client falls back to today's built-in token
  list. Worst case is a slightly-stale help listing for one run, never a broken
  command (dispatch never depends on the manifest).

## Scope
### Phase 1 — manifest contract (framework side)
- [x] Python master: `commands --json` (cheap, no app boot) + real test of shape — PR #79, single-registry refactor; independently re-verified green (97 CLI tests, app/DB-free proof) 2026-07-10
- [x] PHP mirror + test — PR #150 (registry-driven; 10 tests/124 assert; full suite 2778/0) merged v3, independently re-verified
- [x] Ruby mirror + test — PR #19 (full single-registry refactor; 11 specs) merged v3, independently re-verified (73/73 CLI)
- [x] Node mirror + test — PR #18 (full single-registry refactor; 29 manifest + 99 cli) merged v3, independently re-verified
- [x] Confirm identical shape across all 4 (parity test) — same `{framework,version,commands[]}` shape verified in every manifest

### Phase 2 — client becomes manifest-driven (Rust) — DONE 2026-07-10 (feature/cli-phase2-manifest)
- [x] Query `<cli> commands --json` via `resolve_cli()`; cache at `.tina4/commands.json`
      keyed by a cheap stat fingerprint (path+mtime+size) of the resolved framework CLI;
      `--refresh` escape hatch; graceful fallback on spawn/parse failure — `src/manifest.rs`
- [x] Merge discovered commands into help output — `print_help()` in `src/main.rs` intercepts
      `tina4`/`-h`/`--help`/`help`/`--refresh`, renders clap's help, then appends a
      "Discovered from <framework> <version>" block of the manifest commands clap doesn't
      already own. Consumed for the listing ONLY; dispatch never touches it.
- [x] Pass-through dispatch for any non-native command — clap `#[command(external_subcommand)]`
      `External(Vec<String>)` forwards `tina4 <cmd> <args...>` verbatim to `<framework-cli>`.
      Removed the typed Migrate/Test/Routes/Metrics/Generate/Seed/Queue/Console variants (+ the
      `QueueAction` enum) and DROPPED the `migrate --create` -> `migrate:create` translation
      (manifest advertises `migrate:create`; it forwards verbatim now).
- [x] Delete `generate.rs` (generate is now discovered + forwarded)
- [x] Real test: `tina4 generate crud X` produces the framework's output — `tests/scaffold.rs::
      generate_crud_delegates_to_framework` (real global tina4python, PASS). Cache populate/
      reuse/refresh proven end-to-end in `tests/manifest.rs::manifest_cache_populate_reuse_refresh`
      (real v3 tina4python via TINA4_MANIFEST_CLI, PASS) + real-fs unit tests in `src/manifest.rs`.
      Verified: Python only (see note below).

Phase-2 verification notes (2026-07-10, macOS, cargo 1.94.0):
- `cargo build --release`, `cargo test` (86 unit + 3 integration, 0 fail), `cargo clippy -- -D warnings`
  (CI-exact) all GREEN. Gated `#[ignore]` tests run + PASS with `--ignored`.
- Verified `generate crud` delegation against **Python only** — the released global `tina4python`
  was used (it already ships `generate crud`). PHP/Ruby/Node framework CLIs are NOT installed on
  this machine (no `tina4php`/`tina4ruby`/`tina4nodejs` on PATH; they resolve per-project via
  composer/bundle/npx), so their delegation was NOT live-verified. Dispatch is framework-agnostic
  (blind forward), so no code path is Python-specific — but the live proof is Python-only.
- The released global `tina4python` predates the Phase-1 `commands` subcommand, so the manifest
  cache POPULATE path needed a v3 build; verified against a v3 `tina4python` built from
  `tina4-python@origin/v3` into an isolated venv. The FALLBACK path (older CLI, no manifest) is
  what the released CLI exercises: `tina4 --help` still renders, no cache is written, dispatch
  unaffected.

### Phase 3 — bring the frameworks to one command set (parity, now visible as manifest diffs)
- [ ] Ruby: add `migrate:create`
- [ ] Python / Ruby / Node: add top-level `queue` command (wire to Queue subsystem)
- [ ] `build`: decide per-framework semantics; fix Python's `python -m build` (packages the lib, not the app)
- [ ] Re-run manifest parity test — all 4 identical

### Phase 4 — every code-producing generator co-emits a real test (owner req 2026-07-10)
Today only `crud` bundles a test (via `_gen_test`, which is genuinely real:
real TestClient + JWT + SQLite). Extend that to every generator that emits code.
Generated tests must be REAL (no mocks — exercise the scaffold against real
SQLite / real TestClient / real Queue) and GREEN on generation (they test the
scaffold, which works — a scaffolded test that fails on creation is bad DX).
- [ ] Python master: model/route/middleware/service/queue/validator/seeder/websocket/listener each co-emit a real test alongside their code (reuse the `_gen_test` real-collaborator pattern)
- [ ] Decide: migration co-emits an up/down-applies test? auth co-emits login/register tests? (recommend yes)
- [ ] PHP / Ruby / Node parity — same generators co-emit `*Test.php` / `*_spec.rb` / `*.test.ts`
- [ ] Verify: scaffold each into a real project and run its generated test green, per framework (no mocks)

### Verify
- [ ] Independent: live-run each discovered command against a real project per framework (no mocks)

## Parity dashboard
| Capability | Python | PHP | Ruby | Node | Client |
|---|:--:|:--:|:--:|:--:|:--:|
| `commands --json` manifest | ✅ #79 | ✅ #150 | ✅ #19 | ✅ #18 | ✅ consumed (help) |
| generate (delegated) | - | - | - | - | ✅ forwarded (verified Python) |
| queue command | add | OK | add | add | discovered |
| migrate:create | OK | OK | add | OK | discovered |

## Risks / Open questions (need owner ruling)
1. **Manifest command name** — `commands --json`? (recommended: human `commands`
   lists them, `--json` = machine form). It must NOT collide with anything and
   must be cheap.
2. **Caching strategy** — cache in `.tina4/commands.json` keyed by framework
   version, refresh on version change (recommended) vs query every run (simpler,
   slower) vs query only on help/unknown (fastest, but stale help).
3. **`build`** — with the manifest model the client only forwards `build` if the
   framework advertises it; SCSS stays client-side. Still worth fixing Python's
   `build` (currently packages the library). Ruby/Node: define or omit.

## Decisions (locked 2026-07-10)
1. Manifest command: `commands` (human) + `commands --json` (machine).
2. Caching: `.tina4/commands.json`, validated by a cheap `stat` fingerprint of
   the resolved framework CLI (path+mtime+size), `--refresh` escape hatch,
   graceful fallback. Dispatch never needs it (native-set-owned + blind forward).
3. `build`: client keeps SCSS; forwards `build` only if advertised; fix Python's
   `python -m build` (packages the library, not the app).
4. Every code-producing generator co-emits a real, green, no-mock test
   (model/route/middleware/service/queue/validator/seeder/websocket/listener +
   crud already + auth + migration up/down). Only `test` itself is exempt.

## Status: In Progress — Phase 1 COMPLETE (all 4 `commands --json` manifests merged to v3: Py #79 · PHP #150 · Rb #19 · Node #18, each independently re-verified). Phase 2 COMPLETE on `feature/cli-phase2-manifest` (Rust client: manifest consumer + fingerprint cache + `--help` merge + blind pass-through dispatch + `generate.rs` deleted; delegation live-verified Python-only). Phase 3 (framework command-set parity: Ruby `migrate:create`, top-level `queue`, `build` semantics) NEXT. 2026-07-10
