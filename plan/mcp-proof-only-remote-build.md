# Thread 11 — Proof-only MCP: let a remote AI build locally without seeing the code

## The concept to VALIDATE (one falsifiable claim)
> A registered external AI engine (Claude / Copilot / Grok), given MCP access to
> a local dev project **over an ngrok tunnel**, can trigger a real build and read
> back **proof that it works** — while the SOURCE, `.env`, secrets and data
> **never leave the machine**.

If a curl from "outside" (the ngrok URL) can scaffold+verify a resource and get
`{created, tests, endpoints, rendered}` back with **no file bodies and no
secrets**, AND a source-exposing tool is refused on that surface — the concept
holds. If proof can't be produced without shipping source, it doesn't.

## What already exists (verified on the running playground)
- Local MCP server at `/__dev/api/mcp/{tools,call}` + streamable
  `/__dev/api/mcp/endpoint|message|sse`. Tools registered via `@mcp_tool`
  (registry in `tina4_python/mcp/__init__.py`).
- 20 live tools — BUT several **leak**: `file_read`, `file_write`, `file_patch`,
  `database_query`, `template_render` return source/data. These must NOT be on
  the remote surface.
- The whole verification ladder (scaffold-first, import/symbol/execution/render
  verify, tests, rollback) already runs in the Rust agent — it just isn't a tool
  yet. That ladder IS the proof generator.
- ngrok available for the tunnel (user has it).

## The gap
1. No single **proof-returning build tool** — the capability lives in the agent's
   build loop, not as an MCP tool.
2. No **remote-safe vs local-only** classification, so the tunnel would expose
   `file_read` etc.
3. No **registration/token gate** distinguishing a local caller from a tunneled
   remote one.

## Scope (tiered — validation FIRST)

### Slice 0 — CONCEPT VALIDATION (smallest falsifiable test)
- [ ] Add ONE tool `tina4_scaffold_verify(kind, name, fields?)` that runs the
      existing generate + tests + endpoint-smoke and returns PROOF ONLY:
      `{ok, created:[filenames], test_summary, endpoints:[{path,status}],
        contains_source:false}`. NEVER a file body, DDL, row, or secret.
- [ ] Tag it `remote_safe: true`; tag `file_read/file_write/file_patch/
      database_query/template_render` `remote_safe: false`.
- [ ] A tunneled caller (marked by a header/token) may call only `remote_safe`
      tools; a local caller keeps everything.
- [ ] EXPERIMENT: expose the playground MCP via `ngrok`; from outside the box
      call `tina4_scaffold_verify` → assert the JSON has proof and **zero
      source** (grep the response for any known code token → none); call
      `file_read` over the tunnel → assert **refused**.

### Slice 1 — the proof contract + more capabilities
- [ ] `tina4_build_plan(plan)` (proof-only wrapper over `/execute`),
      `tina4_render(page)` (returns "mounted + N rows", not HTML).
- [ ] A response linter: a `remote_safe` tool's output is scanned and rejected
      if it contains a file body / secret pattern before it leaves.

### Slice 2 — tunnel + registration (Rust)
- [ ] Rust agent manages the ngrok-type tunnel; each registered dev gets a
      wildcard subdomain `<user>.dev.tina4.com` → their local `/__dev/mcp`.
- [ ] Registration token auth; per-token tool allow-list.

## Tests / verification (real)
- [ ] `tina4_scaffold_verify` returns proof; the response contains none of the
      generated file's source lines (assert by substring).
- [ ] `file_read` over a `remote` caller → error; over a `local` caller → works.
- [ ] Live over ngrok: external curl builds + validates; `wireshark`-simple check
      = the response body carries no source.

## Status: 🟡 Scoping — Slice 0 is the concept validation
