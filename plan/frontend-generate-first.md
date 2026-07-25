# Thread 10 — Frontend building: generate-first for tina4-js

## Why this thread
The independent agent builds textbook BACKEND code because generators do the
heavy lifting (`generate model/route`) and the LLM only fills custom logic —
the reuse ladder. The FRONTEND has none of that: no generators, no coder
contract, no grounding. A "build a UI" request today would make the coder
hand-roll tina4-js — and the tina4-js skill exists precisely because
"AI agents consistently get tina4-js patterns wrong" (renders once but never
updates, inputs don't bind, `${false}` prints "false"). So the SAME move that
made the backend work is the move here: **give tina4-js scaffolding, then the
coder scaffolds first and fills only logic.**

## Ground truth (verified, not memory)
- tina4-js API (IIFE global `Tina4`): `signal, computed, html, effect, batch,
  Tina4Element, api, ws, sse, route, router, navigate, pwa`. Source of truth =
  the installed `tina4-js` skill + `tina4-js/src`.
- Distribution: `dist/tina4js.min.js` (~27.7KB IIFE) exposes everything on
  `window.Tina4`; `<script src="/js/tina4js.min.js">` — no build step needed.
- A tina4 backend serves everything from `public/`; projects ship a `frontend/`
  dir. `npx tina4js create` scaffolds a whole ES-module PROJECT (+ a Python
  catch-all `spa.py`) — but there is NO per-piece generator.
- The playground currently serves only the 2.8KB `tina4.min.js` CORE, not the
  full bundle — a real page needs the full `tina4js.min.js` served too.

## The gap
1. No `tina4js generate page/component` — the frontend equivalent of
   `tina4python generate model/route`.
2. Coder has ZERO frontend guidance (`TINA4_CODER_CONTRACT` is 100% backend and
   forbids other frameworks but never mentions tina4-js).
3. Grounding retrieves backend API, not signals/html/components.
4. Verification smokes API endpoints; nothing RENDERS a page.

## Scope (tiered)

### Slice 1 — scaffolding + first rendered page  ✅ DONE (tina4-js `bin/tina4.js`)
- [x] `tina4js generate page <name> [--api /api/x]` — idiomatic reactive page
      (no-build IIFE, global `Tina4`): `signal` list, `api.get`, reactive block
      `${() => rows.value.map(...)}`, `router.start({target:'#root'})`.
- [x] `tina4js generate component <Name>` — a `Tina4Element` (hyphenated tag,
      `static styles` scoped CSS, signal reads kept inside `${}` holes).
- [x] `ensureBundle` copies the full `tina4js.min.js` when only the core is
      present; `ensureCss` ensures `tina4.min.css` is served.
- [x] Emits an HTML shell (bundle + `tina4.min.css` + page + `#root`).
- [x] RENDERED live: `/products.html` mounted, listed the real `/api/products`
      row; add a product + Refresh updated the list IN PLACE (reactive, not
      render-once); zero console errors. 8 generator tests + full suite 335 pass.

## STYLING RULE (non-negotiable)
Frontend styling uses **tina4-css classes only** (`container/card/table/btn/
alert/...` from `/css/tina4.min.css`). **Inline `style=` is a hard NO.**
Components scope CSS via `Tina4Element.static styles` (shadow DOM), never inline.
The generator bakes this in; a test asserts `style=` never appears.

### Slice 2 — wire the coder (separate thread)
- [ ] `TINA4_FRONTEND_CONTRACT` (mirror of the backend contract): which APIs,
      the binding rules, file placement (`frontend/` or `public/js/`, never
      `src/routes/`), no React/Vue.
- [ ] Detect a frontend/page/component/UI request; scaffold-first via the new
      generators; ground against the tina4-js skill.
- [ ] Render-verify replaces endpoint-smoke for frontend files.

## Tests / verification (real — no mocks)
- [ ] Generator unit: `generate page products --api /api/products` writes a file
      containing `signal(`, `api.get(`, `html\``, and mounts to `#root`; no
      `${false}` conditional, no React import.
- [ ] Live: generate the page, serve it, open in a headless browser →
      `#root` contains the product rows fetched from the live `/api/products`.

## Status: 🟡 Slice 1 in progress
