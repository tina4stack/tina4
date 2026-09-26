# Task: CLI generate/routes dispatch fixes + generator-parity gate

Outcome: `tina4 routes` loads project route files, `tina4 generate page|component`
works in a js project, and a committed generator-parity CI gate guards produced
names/paths across python/php/ruby/nodejs. Verified against tina4 CLI 3.8.x +
frameworks 3.13.13x on the lab.

## Root-cause triage (reproduced for real on the lab — no guessing)

| Bug | Via `tina4` | Via framework CLI directly | Lives in |
|-----|-------------|----------------------------|----------|
| 1 crud double-plural (`orderss.py`) | reproduced | reproduced (`uv run tina4python generate crud Order`) | **framework** generator (`tina4-python/tina4_python/cli/__init__.py:1973` `route_name = table + "s"`) — NOT the CLI |
| 2 `routes` "No module named 'src'" | reproduced | reproduced; `PYTHONPATH=.` fixes it | framework routes loader; **CLI-fixable** by exporting the project root on delegation |
| 3 js `generate` -> `vite generate` fails | reproduced | `npx tina4js generate` works | **Rust CLI** dispatch (`detect.rs` cli_name tina4js = "vite"; resolve_cli catch-all) |

The CLI forwards `generate` verbatim to the framework CLI (see CLAUDE.md +
tests/scaffold.rs). Bug 1 is therefore structurally a framework generator bug and
cannot be fixed in the CLI without reimplementing generation (violates reuse
ladder + parity). Documented + guarded by the gate; framework PR is the fix.

## Scope
- [x] Bug 3: `resolve_cli` delegates tina4js `generate`/external to `npx tina4js`
- [x] Bug 2: `delegate_command` puts the project root on PYTHONPATH (python) so routes load
- [x] Rust tests for bug 2 + bug 3 (real binary, gated where a toolchain is needed)
- [x] Generator-parity gate: fixture + runner script + matrix workflow
- [x] Gate FAILS on today's bugs (proven) and passes for CLI-owned bugs after fix
- [ ] PR into main; CI green

## Parity
| Item | Python | PHP | Ruby | Node | tina4js |
|------|--------|-----|------|------|---------|
| routes loads (bug2) | ❌→ | n/a-check | n/a-check | n/a-check | n/a |
| generate dispatch (bug3) | ✅ | ✅ | ✅ | ✅ | ❌→ |

## Tests (real, no mocks)
- [ ] `generate page` in a js project resolves to `npx tina4js` not `vite`
- [ ] `routes` after `generate crud` lists the generated routes (no import error)
- [ ] parity gate script asserts the naming contract + fails on the 3 bugs

## Bugs
- [ ] bug2 routes PYTHONPATH
- [ ] bug3 tina4js dispatch
- [ ] bug1 framework double-plural (framework PR — out of CLI scope; gate guards it)

## Commits
- 479997f  bug2 PYTHONPATH + bug3 tina4js dispatch + generator-parity gate (verified lab: py/php/tina4js gate pass patched, fail unpatched)

## Status: In Progress
