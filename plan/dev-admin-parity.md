# Thread 6 — Dev-admin endpoint parity across frameworks

The shared SPA ships to all 4 frameworks. Every `/__dev/api/*` it calls must
work identically on python/php/ruby/nodejs. Audit (3 parallel tina4-dev agents,
2026-07-12) vs the python reference dev_admin.

## Headline
Route-level parity is complete — **62/62 reference endpoints present** in PHP,
Ruby, and Node. The gaps are (a) a shared SPA↔backend **contract drift** and
(b) **present-but-inert stubs**. Notably **Node is the most complete**, and
**python (the reference) is behind the SPA contract**.

## Parity matrix — the SPA-visible gaps

| Gap | Python | PHP | Ruby | Node | SPA impact |
|-----|--------|-----|------|------|------------|
| `POST /migrate` | ❌ (via /tool) | ❌ | ❌ (stub) | ✅ real | Migrate chip **404s** on py/php/ruby |
| `POST /test` | ❌ (via /tool) | ❌ | ❌ (stub) | ✅ real | Test chip **404s** on py/php/ruby |
| `POST /seed/run` (project seeder) | ❌ | ❌ | ❌ | ✅ real | Seed chip **404s** on py/php/ruby |
| `GET /grounding/status` + `POST /grounding/token` | ❌ | ❌ | ❌ | ✅ real | 🔑 panel works in dev (vite proxy) but **breaks in prod** on py/php/ruby |
| `GET /table` (columns+rows) | ✅ real | ✅ | ✅ | ❌ stub (empty) | DB table detail empty on Node |
| queue retry/purge/replay | ✅ real | ✅ | ❌ stub (no-op) | ⚠ in-memory | Ruby buttons report success, do nothing |
| websockets / websockets/disconnect | ✅ real | ✅ | ❌ stub | ✅ | Ruby WS panel always empty |
| `POST /tool` (test/migrate/seed) | ✅ runs | ⚠ | ❌ stub | ❌ stub | generic runner inert on ruby/node (mitigated by dedicated routes on node) |

Note: `POST /seed` (table seeder, used by Database.ts) IS present everywhere —
distinct from `/seed/run` (project seeder chip).

## Scope (tiered — pick per the nod)

### Tier 1 — make the shipped chips work everywhere (hard 404 fix)
- [ ] `POST /migrate`, `POST /test`, `POST /seed/run` in **python, php, ruby**
      (node already real). Reuse each framework's real migrate/test/seed path
      (python: `cli._migrate` / pytest; php: composer; ruby: rake/bundle).
- [ ] Report result shape the SPA's `summariseRun` expects.

### Tier 2 — 🔑 grounding panel in production
- [ ] `GET /grounding/status` + `POST /grounding/token` in python, php, ruby —
      proxy to the Rust agent's `/mcp/{status,token}` (as node does), or write
      `TINA4_MCP_TOKEN` to `.env` directly.

### Tier 3 — fill the inert stubs (behavioral parity)
- [ ] Ruby: real queue retry/purge/replay, websockets enumerate + disconnect,
      `/tool` actually runs test/migrate/seed.
- [ ] Node: `/table` real columns+rows; consider file-backed queue read.

## Tests (real — no mocks)
- [ ] Per framework: boot `tina4 serve` (TINA4_DEBUG), POST /migrate|/test|/seed/run
      → 200 + real effect (migration applied / tests run / seed inserted).
- [ ] Grounding: GET /grounding/status returns configured flag; POST token persists.
- [ ] Ruby queue: enqueue a job, POST /queue/purge → job actually gone.

## Bugs
- [ ] py/php/ruby: scaffold chips 404 (dedicated /migrate,/test,/seed/run absent).
- [ ] py/php/ruby: 🔑 grounding panel dead in production.
- [ ] ruby: queue/websockets/tool are 200-but-no-op stubs.
- [ ] node: /table returns canned empty payload.

## Verification (all four, real — booted `tina4 serve` per framework)
- [x] **Python** — /migrate `{applied,skipped,failed}` idempotent, /test real exit code,
      /seed/run 5 rows, grounding flips + .env. Reused `Migration` + `seed_models`.
- [x] **PHP** — same 5 routes verified (phpunit pos+neg, seed via SELECT COUNT).
      Existing 95/95 dev-admin tests still green. Reused `\Tina4\Migration::migrate()`.
- [x] **Ruby** — all 3 tiers; real queue purge/retry/replay, real websocket
      enumerate+disconnect, /tool de-stubbed. New spec `dev_admin_run_chips_spec.rb`.
      Full suite **3873 examples, 0 failures**.
- [x] **Node** — /table real columns+rows (with identifier guard), /queue +
      dead-letters now file-backed. New test `devAdminDbQueue.test.ts` (18 pass),
      smoke 107 pass, tsc clean. Grounding already matched (proxies to agent).

## Follow-up bug (found + fixed this thread)
- [x] **Ruby `/queue` stats all-zero** — handler called `queue.size("pending")`
      positionally but `Queue#size` is keyword-only → `ArgumentError` swallowed by
      the rescue. Fixed to `size(status:)` + real regression test. Parity checked:
      python/php/node use positional-with-default signatures (`size(status="pending")`)
      so the bug is **Ruby-only** — no port needed.

## Grounding — uniform across all four (resolved)
All four frameworks now write/read `TINA4_MCP_TOKEN` in the project `.env`
directly — standalone, no Rust-agent dependency. Node originally proxied to the
agent's `/mcp/{status,token}`; it was made self-contained to match py/php/ruby
(same `{configured,last4,url}` wire shape; agent still resolves the token at run
time). Node test extended with a real .env round-trip (29 pass).

## Status: ✅ Complete — full parity, all four verified live.
