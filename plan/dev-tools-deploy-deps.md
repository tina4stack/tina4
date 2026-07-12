# Thread 7 — Dev tools deploy dependencies (correctly)

## Goal
The dev-admin's `POST /__dev/api/deps/install` should add a dependency using the
project's REAL package manager so it **persists to the manifest** and lands in
the project env — and support **dev** dependencies (pytest, etc.).

## Context / findings
- The endpoint exists but the Python path runs `pip install` — in a `uv`-managed
  project that neither updates `pyproject.toml` nor installs into the project
  venv (it can hit the wrong env). Ruby runs `gem install` — same problem, no
  `Gemfile` update.
- `npm install` (node) and `composer require` (php) already persist. Good.
- This is a framework change (each `dev_admin`), not the agent.

## Scope
- [x] Python: prefer `uv add` (falls back to `pip install` if uv/pyproject
      absent). Support a `dev` flag → `uv add --dev <pkg>`.
- [x] **`tina4python init` now scaffolds `pyproject.toml`** — a Tina4 project is a
      uv project (the Dockerfiles already `COPY pyproject.toml` + `uv sync`, but
      init never wrote one). Ships pytest in the dev group so scaffolded tests run
      from the first commit. This is what makes `uv add` / `uv run pytest` work.
- [x] Ruby: prefer `bundle add` (falls back to appending to the Gemfile).
      `--group development` for dev.
- [x] Node/PHP: dev flag (`npm install --save-dev` already present; PHP now
      `composer require --dev`).
- [x] SPA: a "dev dependency" checkbox on the Dependencies panel, threaded to
      `POST /deps/install {dev}`.
- [x] Parity: python (uv), php (composer --dev), ruby (bundle add), node (--save-dev).

## Tests (real — no mocks)
- [x] `tina4python init` → `pyproject.toml` present; `uv sync` resolves it.
- [x] Live app: `POST /deps/install {name:"pytest-mock", dev:true}` → `uv add --dev`
      ran; `pyproject.toml` dev group gained `pytest-mock>=3.15.1`.
- [x] Live app: `POST /deps/install {name:"httpx"}` (no dev) → landed in runtime
      `dependencies`, NOT the dev group.
- [x] Negative: unknown package (`tina4-zzz-nope-9999`) → HTTP 500, clean uv
      "No solution found" error surfaced.

## Bugs
- [x] Python deps install used `pip` (no persist) in a uv project → now `uv add`.
- [x] Ruby deps install only appended to the Gemfile (no install) → now `bundle add`.
- [x] `tina4python init` never created a manifest → the Dockerfiles' `uv sync` had
      nothing to sync; deps had nowhere to persist. Fixed by scaffolding pyproject.

## Verification
- Verified end-to-end in a fresh `tina4python init` project running on 7133 with
  the editable local source: init → pyproject.toml → `uv sync` → three live
  `deps/install` calls (dev / runtime / negative) all behaved correctly.

## Status: ✅ Complete — dev tools deploy dependencies (persisted, dev-aware, parity).
