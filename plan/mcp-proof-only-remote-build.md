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

## Where the MCP lives: ON THE SUPERVISOR (the Rust agent)
The MCP server is built **into the supervisor**, not (only) the framework. Why:
the supervisor already owns generate-first AND the full verification ladder
(import/symbol/execution/render verify, tests, rollback) — so it can expose ONE
high-level, proof-returning `build` tool. The framework's `/__dev/mcp` is
lower-level (20 per-tool endpoints, several of which leak source). A remote AI
should talk to the orchestrator, not the file system.

Topology (the supervisor already USES the local MCP as its private toolbox):

    Remote AI          ──MCP over ngrok──▶   SUPERVISOR              ──MCP (local)──▶  Framework /__dev/mcp
    (Claude/Copilot/                         (Rust agent)            file_read/write,
     Grok)            ◀──── PROOF only ────   scaffold-first +       scaffold, db,
                            (no source)       verification ladder    migrations …
                                                                     (source-exposing —
                                                                      LOCAL ONLY)

The privacy boundary is the supervisor: it consumes the FULL local MCP (incl.
source-touching tools) INSIDE the box, and exposes a SEPARATE, proof-only MCP
surface OUTWARD. The leaking tools are never on the remote surface because they
live on the local MCP the supervisor calls, not on the one it publishes.

## What already exists (verified)
- Supervisor (Rust agent) HTTP surface: `/execute`, `/chat`, `/threads`,
  `/mcp/status|token`, grounding proxy. It runs the whole build+verify loop —
  but exposes NO MCP tool interface yet (only these bespoke endpoints).
- Framework local MCP at `/__dev/api/mcp/{tools,call}` + streamable transport,
  `@mcp_tool` registry (`tina4_python/mcp/__init__.py`), 20 tools — several
  (`file_read/write/patch`, `database_query`, `template_render`) LEAK source/data
  and must never be on a remote surface. (Kept for LOCAL callers only.)
- The verification ladder IS the proof generator — it already reports
  `{files, tests, endpoints, rendered}` in the `/execute` SSE stream.
- ngrok available for the tunnel (user has it).

## The gap
1. The supervisor has **no outward MCP surface** — only bespoke HTTP endpoints.
   It needs to PUBLISH an MCP server (JSON-RPC/streamable) with proof-only tools.
2. No single **proof-returning build tool** wrapping its build loop.
3. No **registration/token gate** for a tunneled remote caller.
(The "gate the leaky tools" problem dissolves: they stay on the LOCAL MCP the
supervisor consumes; they are simply never published on the outward surface.)

## Scope (tiered — validation FIRST)

Registration gate (Slice 2): the supervisor MCP can only be UNLOCKED by a
REGISTERED user. Handshake → dev-admin consent → per-session hash lock, and the
whole surface is inert unless the dev is registered. Unregistered = no unlock.

### Slice 0 — CONCEPT VALIDATION ✅ DONE (feature/supervisor-mcp)
Published `POST /mcp/rpc` on the supervisor (JSON-RPC tools/list + tools/call),
one proof-only tool `tina4_scaffold_verify`. Validated on localhost:
- tools/list published ONLY `tina4_scaffold_verify` — `file_read` /
  `database_query` absent from the outward surface.
- tools/call built a real Invoice resource → `{ok:true, created:[6 files],
  test_summary:"22 passed", endpoints:[/api/invoices→200], source_bytes:0}`.
- A grep of the FULL response for `IntegerField`/`class Invoice`/`async def`/
  `CREATE TABLE`/`import` → nothing. Proof crossed; source did NOT.
Additive (no existing route touched); 154 tests + clippy green.

### Slice 0 (original scoping below) —
- [ ] Publish a minimal MCP server ON THE SUPERVISOR (JSON-RPC `tools/list` +
      `tools/call`) with ONE tool: `tina4_scaffold_verify(kind, name, fields?)`.
      It reuses the supervisor's existing scaffold-first + tests + endpoint-smoke
      and returns PROOF ONLY:
      `{ok, created:[filenames], test_summary, endpoints:[{path,status}],
        source_bytes: 0}`. NEVER a file body, DDL, row, or secret.
- [ ] The outward surface publishes ONLY proof tools — it does NOT re-expose the
      local MCP's `file_read`/`database_query`. (Verify `tools/list` over the
      tunnel omits them entirely.)
- [ ] EXPERIMENT: expose the supervisor's MCP via `ngrok`; from OUTSIDE the box
      `tools/call tina4_scaffold_verify` → assert the JSON is proof + **zero
      source** (grep the whole response for a line of the generated file → none);
      `tools/list` → assert `file_read` is absent from the remote surface.

### Slice 1 — the proof contract + more capabilities
- [ ] `tina4_build_plan(plan)` (proof-only wrapper over `/execute`),
      `tina4_render(page)` (returns "mounted + N rows", not HTML).
- [ ] A response linter: a `remote_safe` tool's output is scanned and rejected
      if it contains a file body / secret pattern before it leaves.

### Slice 2 — tunnel + consent auth (Rust + dev-admin)
- [ ] Rust agent manages the ngrok-type tunnel; each registered dev gets a
      wildcard subdomain `<user>.dev.tina4.com` → their supervisor MCP.
- [ ] AUTH = human consent, no token juggling: when a remote AI (Copilot/Grok)
      sends the MCP `initialize` handshake, the supervisor HOLDS it pending and
      prompts IN THE DEV-ADMIN ("Copilot wants to connect — Allow / Deny"). The
      user clicks Allow; the connection proceeds. Per-connection, revocable.
- [ ] On Allow, a per-SESSION hash is generated and the session is LOCKED to it:
      every subsequent MCP call must carry that hash (bound to the approved
      connection). Revoke = drop the hash. No long-lived tokens.
      (Slice 0 is localhost-only — bound to 127.0.0.1, unreachable without the
      tunnel — so it needs no auth.)

### Slice 3 — supervisor reaches OUT (the AI as reasoning intermediate)
Bidirectional: not just AI→supervisor (tools), but supervisor→AI. In MCP terms
this is **sampling** — the server sends `sampling/createMessage` to the connected
client and borrows its LLM. The supervisor becomes the orchestrator that owns the
project + verification ladder and delegates THINKING to whatever brain is
connected.
- [ ] Bidirectional transport (streamable/SSE) so the supervisor can initiate a
      message mid-session (Slice 0's HTTP req/resp can't).
- [ ] `sampling/createMessage` from the supervisor when it needs reasoning
      (plan a step, repair a hallucinated symbol), degrading to `long_context`
      when the client doesn't support sampling.
- [ ] The build LOOP: supervisor asks the connected AI to reason → scaffolds →
      verifies → on failure sends the REAL error back and asks again. The AI
      reasons; the supervisor keeps it honest with proof each turn.
- [ ] OUTBOUND privacy: what the supervisor SENDS the AI is minimal context (a
      step, an error, a status) — NEVER the codebase or secrets. Proof-only in;
      context-only out.

## Validation scenarios (the concept, proven from several angles)
- Slice 0: localhost curl acting as an MCP client → proof, zero source.
- **Local Ollama → supervisor MCP**: a fully-OFFLINE proof — a local Ollama model
  is the AI engine calling the supervisor's proof-only tools. Nothing leaves the
  box at all; the strongest form of the privacy claim.
- Cloud AI (Claude/Copilot/Grok) over ngrok → proof, zero source (Slice 2).

## Tests / verification (real)
- [ ] `tina4_scaffold_verify` returns proof; the response contains none of the
      generated file's source lines (assert by substring).
- [ ] `file_read` over a `remote` caller → error; over a `local` caller → works.
- [ ] Live over ngrok: external curl builds + validates; `wireshark`-simple check
      = the response body carries no source.

## tina4-js MCP module (user direction)
Build the MCP capability into **tina4-js** as a first-class module, alongside the
existing client modules (`api`, `ws`, `sse`). Two possible shapes — CONFIRM which:
- **`mcp` CLIENT** (`Tina4.mcp`): a browser/JS MCP client so any tina4-js app —
  starting with the dev-admin SPA — connects to the supervisor's proof-only MCP
  and calls `tools/list` / `tools/call`. Mirrors `api`/`ws`/`sse`. (Most likely.)
- **`mcp` SERVER helpers**: JS-side building blocks for exposing MCP tools, if a
  Node/tina4-js supervisor should PUBLISH the surface (vs the Rust agent).

DECIDED: the proof-only SERVER lives in the **Rust supervisor** — most efficient
(it already owns the verification ladder; no new runtime; the proof payload it
emits IS the tool result). The tina4-js `mcp` CLIENT module is a later slice so
the dev-admin SPA (and any tina4-js app) can call it.

## Status: 🟡 Building Slice 0 in the Rust supervisor
