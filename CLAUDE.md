# Tina4 CLI

Version 3.8.43 — Unified CLI for Python, PHP, Ruby, and Node.js Tina4 frameworks.

## Build & Test

- Language: Rust (2021 edition)
- Build: `cargo build --release`
- Test: `cargo test`
- Install: `cargo install tina4` or download from crates.io
- Update: `cargo install tina4 --force`

## Commands

```
tina4 setup                      Guided menu: language + AI tool + projects folder +
                                 project name, then installs runtime/git/skills/AI tool
                                 (via Chocolatey/Homebrew), scaffolds the project with its
                                 own CLAUDE.md + .mcp.json. Claude Code (the default) opens a
                                 real seeded session in the project; Claude Desktop/none get a
                                 "start it now?" prompt (`tina4 serve` → opens app + /__dev).
                                 --dry-run = preview only. --skip-install = scaffold, no installs.
tina4 init <language> <path>     Scaffold a new project (python, php, ruby, nodejs, tina4js)
tina4 serve [project]            Start dev server (file watcher + SCSS + browser). With a
                                 project name, cd into ./<name> (then the configured projects
                                 folder) and serve that project. Bare `tina4 serve` outside a
                                 project falls back to the configured projects folder: one
                                 project there → cd in + serve it; several → list them; none →
                                 guidance. cd into the resolved project happens automatically.
tina4 serve --production         Auto-install and use production server
tina4 serve --no-browser         Don't open browser on startup
tina4 doctor                     Check installed languages and tools
tina4 install <target>           Install a language runtime or tina4-js
tina4 generate <what> <name>     Generate model, route, migration, middleware
tina4 migrate                    Run database migrations
tina4 test                       Run tests
tina4 routes                     List registered routes
tina4 scss                       Compile SCSS files
tina4 ai                         Detect AI tools and install context
tina4 update                     Self-update the binary
```

## Windows `tina4 setup` — the "drops to the prompt" symptom (RESOLVED in 3.8.40)

**Root cause (confirmed from a real Windows screenshot, 3.8.39):** the installer
is run as `irm https://tina4.com/install.ps1 | iex`. That makes the PowerShell
host's stdin the *download pipe* — already at EOF. `install.ps1` then chained
straight into `& "$dest" setup`, an interactive wizard. With stdin dead, every
prompt silently defaulted and then the UAC elevation fired from a
non-interactive context and failed → `exit 1` → install.ps1's catch printed
"Setup didn't finish." No menu ever appeared.

**Fix (3.8.40), two parts:**
1. **`install.ps1` / docs-site copy** no longer auto-launch setup. It installs
   the binary and prints `Next step — run: tina4 setup`. The user runs it in
   their own fresh terminal, where stdin is a real console.
2. **`setup.rs` has a stdin-TTY guard** (`io::stdin().is_terminal()`): a
   non-interactive stdin prints "Setup is interactive — open a new terminal and
   run: tina4 setup" and exits **0** (not a scary failure). The elevated re-run
   (`TINA4_SETUP_ELEVATED`) and `--dry-run` / `--skip-install` are exempt.

`install.sh` was already correct — it runs `tina4 setup < /dev/tty`, so the Mac
`curl | sh` flow keeps its single-console auto-launch. The TTY guard passes
there because `/dev/tty` is a terminal.

**Historical handoff detail (the elevation design, still in place):**

**macOS is the reference flow and works end-to-end** (verified): `tina4 setup`
asks the menu in-console → scaffolds the project → `uv sync` → `tina4 serve`
returns HTTP 200. Mac never elevates, so it all happens in one console. Match
this single-console experience on Windows.

**The Windows symptom (reported by Andre):** `tina4 setup` prints "Starting
setup…" then **drops straight back to the prompt** — the wizard never appears.

**Root cause:** `choco install` needs Administrator. The old code elevated
*before* the menu and relaunched the whole wizard into a **new elevated window**
via `Start-Process -Verb RunAs`, exiting the original console → looks like it
"just dropped to the prompt", and the menu is stranded in a window you may not
see (or the relaunch was declined/failed).

**What's already done (v3.8.38, in `src/setup.rs`):**
- The menu (`choose_language`/`choose_ai`/`choose_projects_dir`/name) now runs
  **in the user's console first** — `elevate_for_install()` is called only
  *after* the answers are collected (was `ensure_admin_windows()` up front).
- Elevation passes the answers to the elevated re-run via env
  (`TINA4_SETUP_ELEVATED` + `TINA4_SETUP_LANG`/`_AI`/`_DIR`/`_NAME`); the
  elevated instance reads them in `elevated_answers()` and skips the menu.
  `pause_if_elevated()` holds the elevated window open at the end.
- Installers (`install.ps1`/`install.sh`) print the `tina4 setup` command list
  before launching setup + a "run tina4 setup again" hint on non-zero exit.
- Verified under Wine: `--dry-run` shows the menu; the elevated re-run
  (env answers set) skips the menu and uses the passed lang/dir/name. **Wine
  can't exercise real UAC, so the actual elevation path is UNVERIFIED.**

**Still to confirm / likely finish on a real Windows box:**
1. Does `tina4 setup` (non-admin) now show the menu in-console before any UAC?
   Run `tina4 --version` first to be sure you're on **3.8.38** (a stale
   `tina4` earlier on PATH was a prime suspect).
2. `is_admin_windows()` uses `net session` — confirm it correctly reports
   non-admin on the user's box (Wine false-positives admin).
3. Preferred design (mirror Mac's single console): instead of relaunching the
   whole wizard elevated, keep the **wizard + scaffold in the user's console**
   and elevate **only the Chocolatey install** as a short `Start-Process
   -Verb RunAs -Wait` sub-step that returns control to the original console.
4. Test paths: `tina4 setup --dry-run`, `tina4 setup --skip-install` (both skip
   elevation), then the real `tina4 setup`.

**Implemented in 3.8.39 (verified on macOS, needs a real-Windows pass):**
- **Claude Code is the default AI pick** and ends setup by launching a real
  seeded session: `claude "<FIRST_PROMPT>"` in the project dir (`whats_next()`).
  The launch resolves the binary via `which::which("claude")` and — because on
  Windows `claude` is a `.cmd`/`.ps1` shim that `Command::new` can't spawn
  directly — runs it through `cmd /C <resolved-path> "<prompt>"` on Windows,
  bare path elsewhere. **Confirm on a real Windows box** that the session opens
  in the project (not just the fallback "cd … && claude" print).
- **`open_ide()` is opened AFTER the "Start it now?" prompt** (Desktop/none
  path) so the GUI no longer steals terminal focus before the prompt prints.
- **Per-project `.mcp.json`** (`write_project_mcp_json`) wires Claude Code to the
  project's live `/__dev/mcp` tools (port per language).
- **`tina4 serve <project>`** resolves `./<name>` then the configured projects
  folder, `cd`s in, and serves.

## Key Architecture

- Auto-detects project language from app.py/index.php/app.rb/app.ts
- **Sole file watcher** for the Tina4 stack (notify crate). Watches
  `src/`, `migrations/`, `.env`. On a meaningful change it POSTs
  `/__dev/api/reload` to the running framework server — it does NOT
  restart the server. The framework broadcasts the reload signal to
  connected browsers via WebSocket (`/__dev_reload`) with a polling
  fallback (`GET /__dev/api/mtime`).
- **Event filter** (see `src/watcher.rs`): drops Access / Metadata-only
  events; ignores `__pycache__`, `.git`, `.venv`, `node_modules`,
  `vendor`, `dist`, `target`, `logs`; ignores `.log`, `.db`, `.db-wal`,
  `.db-shm`, `.sqlite`, `.tmp`, `.swp`, `.pyc` files; does a real mtime
  check to defeat overlayfs / polling-mode spurious events.
- SCSS compilation via grass crate (zero-dep, no sass/node required)
- Port auto-increment if default port is in use
- Cross-platform: macOS, Linux, Windows (ANSI fallbacks for cmd.exe)
- Default ports: PHP 7145, Python 7146, Ruby 7147, Node.js 7148

## Dependencies

- clap: CLI argument parsing
- colored: Terminal colors
- notify: File system watcher
- grass: SCSS compiler
- which: Binary lookup
- ctrlc: Signal handling

## Links

- crates.io: https://crates.io/crates/tina4
- GitHub: https://github.com/tina4stack/tina4
- Website: https://tina4.com

## First Principle: Documentation Matches Code Reality

**This rule overrides everything else in this file.**

Every command, env var, method, class, or feature mentioned in any
documentation file (`*.md` in this repo, or any tina4-book chapter,
or `tina4-documentation/docs/`) MUST exist in code. No exceptions.
No "we'll build it later" entries. No Laravel/Rails-style commands
that look right but don't exist. No env vars that the framework
doesn't actually read.

When you add a doc reference, add the implementation in the same PR.
When you remove a feature, remove every doc reference in the same PR.
When you find drift, fix it both ways: build the real thing OR delete
the doc.

The `tina4-documentation/scripts/audit-truth.py` script is the source
of truth. It runs as a CI gate (`audit-truth.yml`) on every PR — the
build fails on CLI drift. Run it locally before pushing if you've
touched docs:

```bash
cd /path/to/tina4-documentation
python3 scripts/audit-truth.py --strict
```

If you're unsure whether something exists, run `tina4 <command> --help`
or grep the framework source. Don't guess.
