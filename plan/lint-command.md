# Thread — `tina4 lint` (unified linter across all four frameworks)

## Goal
`tina4 lint` runs a lint pass on the project, auto-detecting the language — the
same one-command DX as `tina4 test` and `tina4 metrics`. It adds **zero new
dependencies to any framework**: a runtime-built-in syntax check is the always-on
baseline, and a richer linter (ruff / phpcs / rubocop / eslint) runs ONLY when the
user's own project already has it. `tina4 lint --fix` applies safe autofixes when
the detected tool supports them. Identical UX and exit-code contract in Python,
PHP, Ruby, and Node.

## Zero-dep + supply-chain safety (the two constraints, answered)
Andre's two questions drive the design:
- **"Does this introduce a 3rd-party security risk?"** It must not. Shipping
  eslint / typescript-eslint / phpcs / php-cs-fixer as framework devDeps would
  pull large transitive trees (npm especially) into a framework whose whole value
  is a minimal attack surface. So **`tina4 lint` installs nothing and adds nothing
  to any manifest.** We pull zero new supply chain.
- **"Where does it leave our no-dep — should be opt-in?"** Zero-dep stays intact.
  The baseline uses ONLY what already ships with the language runtime. The richer
  linter is **opt-in, bring-your-own** — the command detects a linter the project
  already installed/configured and runs it; if none is present it runs the
  built-in syntax check and says how to opt in. Never auto-install.

Reuse ladder: rung 3 (the runtime stdlib already syntax-checks) for the baseline,
rung 2/4 (reuse the project's own tool) for the rich pass. Rung 5 ("add a runtime
dependency? almost never") is respected — we add none.

## Why this is cheap on the CLI side (grounding)
The Rust CLI already forwards any command it doesn't own:
`Commands::External(args) => delegate_command(args)` (`src/main.rs:245,442`)
shells `tina4python lint` / `tina4php lint` / ... — exactly how `tina4 test` works
today (there is NO `Test` arm; it is forwarded). Each framework CLI has a
single-source `COMMANDS` registry that drives dispatch, `--help`, AND
`commands --json` (Python: `tina4_python/cli/__init__.py:3745`,
`"test": {"handler": _test, "summary": "Run test suite"}`).

**So `tina4 lint` needs zero new Rust logic** — it works the moment each framework
CLI adds a `lint` handler + registry entry. `lint` is a DELEGATE command
(deliberately unlike native `tina4 metrics` in `src/metrics.rs`; lint and metrics
are complementary — metrics = complexity / MI / DRY, lint = syntax + style).

## The zero-dep baseline — every runtime already ships a syntax checker
| Lang | Baseline (always available, zero dep) | Opt-in rich linter (only if project has it) |
|------|----------------------------------------|---------------------------------------------|
| Python | `python -m py_compile` / `compileall` (stdlib) | `ruff` if on PATH / in the project |
| PHP | `php -l` (ships with php) | `phpcs` / `php-cs-fixer` / `phpstan` if vendored |
| Ruby | `ruby -c` (ships with ruby) | `rubocop` if in the bundle + a `.rubocop.yml` present |
| Node | `node --check` (ships with node) | `eslint` if the project has a config + it installed; `tsc` if typescript present |

Framework manifests gain nothing. tina4-python already lists ruff as ITS OWN dev
tool — that is the framework repo's business, not a dep pushed onto user projects.

## Parity dashboard
| Piece | Python | PHP | Ruby | Node |
|-------|--------|-----|------|------|
| zero-dep syntax baseline wired | ✅ compile() | ✅ php -l | ✅ ruby -c | ✅ node --check / tsc |
| detect + run project's own linter (no install) | ✅ ruff | ✅ php-cs-fixer/phpcs/phpstan | ✅ rubocop | ✅ eslint |
| `_lint` handler in COMMANDS registry | ✅ | ✅ | ✅ | ✅ |
| `--fix` passthrough (when tool supports it) | ✅ | ✅ php-cs-fixer | ✅ rubocop -a | ✅ eslint --fix |
| advertised in `commands --json` | ✅ | ✅ | ✅ | ✅ |
| real tests (clean=0, dirty!=0, --fix, detect) | ✅ 11 green | ✅ 9 green | ✅ 22 green | ✅ 33 green, tsc 0 |

PHP verified INDEPENDENTLY at HEAD (macOS, PHP 8.5.7): CliLintTest alone
OK(9,40); with CommandsManifestTest OK(18,184) once this box's broken pecl
`grpc.so` is neutralized (its startup warning prints to STDOUT and corrupts the
`commands --json` the manifest tests parse — an env defect, reproducible at HEAD,
not the code). tina4Lint faithful; `php -l` via `PHP_BINARY -n -l` (ini-independent);
composer `lint` script repointed to `@php bin/tina4php lint` (was calling the
undeclared phplint); NO dep added. Added the broken-entrypoint + opt-in-hint cases
so PHP matches the canonical set.

**ALL FOUR done + fixture-consistent.** Every framework's lint test now asserts
the identical scenario set (clean / syntax-error-named / --fix / nested-recursion /
entrypoint clean+broken / empty / opt-in-hint / registry+manifest), same idioms
(unclosed-paren syntax error). Remaining OPEN decision: rich-linter coverage —
only Python's env ships the tool so only Python exercises the rich path for real;
recommend ACCEPT (see Fixture parity note).

### Fixture parity (checked 2026-08-29, after Andre asked "are the fixtures consistent?")
Canonical scenario set every framework's lint test must cover — NORMALISED so all
four match (PHP held to it on landing):
clean(baseline)=0 · syntax-error(baseline)=1+file-named · --fix-without-rich-tool=no-op
· nested src/ recursion · entrypoint(app.*) clean+broken · empty=nothing-to-lint ·
opt-in hint on baseline · registry + `commands --json` advertises lint.
- The syntax-error fixture is the SAME idiom in all: an unclosed paren
  (`def oops(` / `function oops(`). Exit contract + summary markers identical.
- Python + Node gained the entrypoint/nested/hint cases Ruby already had
  (Python 8→11, Node 23→33). Re-ran both green.
- **Justified divergence (open decision):** only Python's dev env ships the rich
  linter (ruff), so ONLY Python exercises the rich path (clean/violation/--fix)
  for real; Ruby/Node/PHP prove detection + the zero-dep baseline, and trust the
  rich branch as a thin child-process passthrough (same posture as `test`→pytest/
  rspec, which is also not unit-tested). Accept, OR provision rubocop/eslint/phpcs
  as TEST-ONLY tools in each CI to prove the rich path in all four. Recommend
  ACCEPT (adding the tools, even test-only, cuts against the zero-install point).
- Minor: Node fixtures live in-repo (to resolve the project's own tsc/node_modules);
  the new cases declare their own `package.json {type:module}` so `node --check` is
  hermetic. Python/Ruby use external tmpdirs.

Node verified INDEPENDENTLY at HEAD (macOS): re-ran cliLint (23) + manifest (31)
+ typecheck (0); lint.ts faithful; scope clean; no eslint dep added. Layering:
eslint(opt-in) → tsc --noEmit(TS baseline) → node --check(JS floor).

Ruby verified INDEPENDENTLY at HEAD (macOS, Ruby 4.0.2): re-ran cli_lint_spec +
commands_manifest_spec = 22 examples 0 failures on real `ruby -c`; cmd_lint
faithful; scope clean. rubocop dev-dep dropped from gemspec (Gemfile.lock drift
is gitignored/local-only). One deliberate divergence: Ruby's rich pass needs BOTH
a project `.rubocop.yml` AND a resolvable rubocop (rubocop is noisy without a
config, unlike ruff's sane defaults) — contract/exit/summary still identical.

CLI (`tina4` repo): `tina4 lint` forwards automatically via `External` — proven
end-to-end with the released 3.8.76 binary (clean→exit 0 `[ruff]`, F401→exit 1).
Help text + CLAUDE.md updated. No Rust logic change needed.

Python reference DONE + verified locally (macOS, Python 3.13.5, ruff 0.15.7):
9 lint tests + manifest lock-in green; `tina4 lint` / `tina4 lint --fix` driven
end-to-end through the real binary. PHP/Ruby/Node porting in parallel workers.

## Design decisions (decided from principles; Andre may veto)
- **DEC-1 Delegate, not native.** Rust forwards; each framework CLI owns a `lint`
  handler. No `Lint` arm in `main.rs`. (Consistent with `test`.)
- **DEC-2 Zero new dependency, anywhere.** No linter is added to any framework
  manifest. Supply chain is unchanged. This is the non-negotiable that answers
  the security question.
- **DEC-3 Baseline = runtime built-in syntax check; rich linter = opt-in/BYOL.**
  `tina4 lint` ALWAYS runs the zero-dep syntax check. It ADDITIONALLY runs a
  richer linter only when the project already provides one (detected on
  PATH / vendor / bundle / config), never installing it. No linter present ->
  baseline runs + a one-line "install ruff/eslint/... to enable full linting"
  hint. So the command is useful with zero setup and richer when a team opts in.
- **DEC-4 Contract identical across all four:**
  - `tina4 lint` -> exit 0 clean, non-zero on findings (CI gate, like metrics `--fail-on`).
  - `tina4 lint --fix` -> apply safe autofixes IF the detected tool supports them
    (ruff `--fix`, rubocop `-a`, eslint `--fix`, php-cs-fixer). Syntax-only baselines
    have no fix; report that plainly.
  - Same summary line shape everywhere (e.g. `lint: N issue(s) in M file(s) [tool]`),
    naming which tool actually ran, so output never drifts between frameworks.
- **DEC-5 Scope = the user's app** (`src/` + entrypoint), the code a project dev
  writes — mirroring how `tina4 test` runs the project's tests. The framework
  repos' own linting stays a separate maintainer/CI concern.

## Scope (checklist)
- [ ] Python: `_lint` handler — `py_compile`/`compileall` baseline; run `ruff` if present (`--fix` -> `ruff check --fix`); `"lint"` in `COMMANDS`
- [ ] PHP: `_lint` handler — `php -l` baseline; run `phpcs`/`php-cs-fixer`/`phpstan` if vendored; registry entry
- [ ] Ruby: `_lint` handler — `ruby -c` baseline; run `rubocop` if bundled + configured (`-a` on `--fix`); registry entry
- [ ] Node: `_lint` handler — `node --check` baseline; run `eslint` if project-configured + `tsc` if typescript present (`eslint --fix` on `--fix`); registry entry
- [ ] All four: `lint` appears in `commands --json` so `tina4 lint --help` + discovery work
- [ ] CLI repo: add `tina4 lint` to the help block in `main.rs` + `CLAUDE.md` Commands list
- [ ] Docs: shared `general/` note + `cheatsheet.md` line, stating baseline-is-zero-dep + rich-linter-is-opt-in (docs-match-code gate)

## Tests (written first, REAL — no mocks, positive + negative)
Per framework the "real dependency" is the actual runtime + real fixture files
(no stubbing the checker):
- [ ] clean fixture, no rich linter installed -> `tina4<lang> lint` exits 0, summary names the baseline tool
- [ ] broken-syntax fixture -> non-zero exit, offending file named (baseline alone catches it)
- [ ] project WITH a rich linter (real ruff/rubocop/eslint/phpcs installed in the fixture project) + a style-only violation -> non-zero exit, tool named in summary
- [ ] `--fix` on a fixable fixture (rich linter present) -> re-lint clean AND file changed on disk
- [ ] no rich linter present -> baseline still runs AND the "install X to enable full linting" hint is emitted (not an error)
- [ ] `commands --json` output contains a `lint` entry

## Bugs / cleanups (fix as part of this thread)
- [ ] PHP `composer.json:31` `lint` script calls `phplint`, which is NOT a
      declared dep -> fails on a fresh `composer install`. Fix: repoint it to the
      zero-dep `php -l` sweep (or drop it in favour of `tina4 lint`). Do NOT add
      phplint as a dep.
- [ ] Ruby ships `rubocop` as a gemspec dev dep but no `.rubocop.yml` (declared,
      unused). Decide: leave it optional (lint detects it only when a project
      configures it) or drop the unused dev-dep to stay lean. Either way the
      framework does not force rubocop on user projects.
- [ ] Node has no linter — by design now: `node --check` is the baseline, eslint
      is opt-in. Nothing to add to the framework.

## Verification
Run each framework's new lint tests for real on the lab (.99) at HEAD. Drive
`tina4 lint` / `tina4 lint --fix` end-to-end in a fresh `tina4 init <lang>`
project per language: (a) with no rich linter -> baseline runs + hint; (b) with
the project's own ruff/rubocop/eslint/phpcs installed -> rich pass runs, `--fix`
mutates + re-lints clean. Confirm the forwarded `tina4 lint` reaches the framework
handler. Quote the real summary lines. Ship `feature/release<ver>` -> `v3` -> tag
across all four; bump the CLI help + CLAUDE.md in the `tina4` repo.

## Status: Proposed — awaiting nod. Zero new deps; baseline zero-dep; rich linter opt-in/BYOL.
