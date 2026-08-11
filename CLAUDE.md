# Tina4 CLI

Version 3.8.69 — Unified CLI for Python, PHP, Ruby, and Node.js Tina4 frameworks.

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
                                 own CLAUDE.md + .mcp.json. Claude Desktop (the default) and
                                 "none" get a "start it now?" prompt (`tina4 serve` → opens app
                                 + /__dev, and launches Desktop); Claude Code opens a real
                                 seeded session in the project.
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
tina4 doctor                     Check installed languages/tools, ports, AND global
                                 Tina4 AI-skills currency (~/.claude/skills vs the latest
                                 published ref). Strictly READ-ONLY: it reports + suggests a
                                 refresh; it writes nothing and NEVER touches a project CLAUDE.md.
tina4 install <target>           Install a language runtime or tina4-js
tina4 generate <what> <name>     Generate model, route, migration, middleware
tina4 migrate                    Run database migrations
tina4 test                       Run tests
tina4 routes                     List registered routes
tina4 metrics                    Report code-health top offenders (complexity, large files,
                                 low maintainability, untested, duplication). Flags: --top N, --json,
                                 --fail-on warn|error (CI gate), --path DIR|FILE. NATIVE +
                                 language-agnostic (ADR-0002, src/metrics.rs): scans SOURCE
                                 directly for Python/PHP/Ruby/TypeScript+JS/Rust via tree-sitter,
                                 with NO Tina4 project and NO running framework required.
                                 Formula parity with the Python master (metrics.py) — CC/MI/
                                 thresholds identical, locked by a real parity test.
                                 DRY: cross-file duplicate detection via AST-shape hashing
                                 (Baxter-style), language-agnostic so it covers all five
                                 languages through one code path. Finds Type-1 (exact)
                                 clones plus consistent identifier and same-kind literal
                                 renaming. NOT full Type-2: comments are hashed, so adding
                                 a comment breaks the match (measured in all five
                                 languages, locked by a test). Type-3/4 are NOT detected.
                                 PARSE-HEALTH GUARD: a file the engine cannot read is
                                 REFUSED, never reported and never silently dropped. Two
                                 reasons trigger it, both per file: under 95% of lines
                                 parsing cleanly, or an AST nesting deeper than 800 levels
                                 (which used to abort the whole scan with a stack
                                 overflow). A refused file is excluded from every average,
                                 listed under `unparsed` in --json, counted as
                                 `files_refused` in the summary, and raised as a `warn`
                                 offender so --fail-on warn goes red on it.
tina4 scss                       Compile SCSS files
tina4 ai                         Detect AI tools and install context
tina4 deploy <target> [--runtime R]
                                 Generate deployment scaffolding: docker, systemd,
                                 nginx, cpanel. --runtime is PHP-only and picks the
                                 docker image's server: cli (default, the framework's
                                 own forking server), fpm (nginx + php-fpm, fresh
                                 process state per request), or swoole (openswoole,
                                 app stays resident). Each writes its own companion
                                 files: server.php for swoole, nginx.fpm.conf +
                                 docker-entrypoint.fpm.sh for fpm. --runtime on a
                                 non-PHP project is REFUSED, not ignored.
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

**AI default (3.8.43+): Claude Desktop.** The guided setup targets new /
non-terminal users, so the menu defaults to Claude Desktop (option 1); Claude
Code is option 2. The Desktop/none path ends with a "Start it now?" prompt that
runs `tina4 serve` and opens Desktop via its resolved launcher
(`claude_desktop_exe()` — %LOCALAPPDATA%\AnthropicClaude\claude.exe, never bare
`claude` on PATH). The Claude Code path still launches a seeded session:
`claude "<FIRST_PROMPT>"` in the project dir (`whats_next()`), resolving the
binary via `which::which("claude")` and — because on Windows `claude` is a
`.cmd`/`.ps1` shim `Command::new` can't spawn directly — running it through
`cmd /C <resolved-path> "<prompt>"` on Windows, bare path elsewhere.
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
- **AI-skills currency check** (`doctor.rs`): `install-skills.sh`/`.ps1` record the
  ref they installed in a GLOBAL marker under each target
  (`~/.claude/skills`, `~/.agents/skills`, `~/.cursor/skills` → `.tina4-skills-ref`);
  `tina4 doctor` reads those markers, fetches the current pinned ref from the same
  installer (`https://tina4.com/install-skills.sh`, so "latest" equals what a
  refresh installs), and reports Claude / Codex / Cursor separately
  (current / update-available / offline / not-recorded). Targets:
  `tina4 skills claude|codex|cursor|all`. The path is read-only (curl for the ref,
  no HTTP-client dep) and a refresh only ever writes the chosen home skills dir —
  never a project's CLAUDE.md, AGENTS.md, or repo-local `.cursor/skills` entrypoints.
  The classifier + ref parser are pure and unit-tested.
- Port auto-increment if default port is in use
- Cross-platform: macOS, Linux, Windows (ANSI fallbacks for cmd.exe)
- Default ports: PHP 7145, Python 7146, Ruby 7147, Node.js 7148
- **Child supervision + clean shutdown.** `serve` runs the language dev
  server as a child in its OWN process group (`setpgid`), so a stale tree
  can be group-killed (avoids EADDRINUSE). Because the child is in its own
  group, a signal to the CLI never reaches it on its own. The shutdown
  handler (`ctrlc` with the `termination` feature: SIGINT, SIGTERM, SIGHUP)
  explicitly `killpg`s the whole `npx -> tsx -> node` (or `uv -> python`,
  `bundle -> ruby`) tree. Without this the Node `npx -> tsx -> node` chain
  leaked one orphaned `node app.ts` per shutdown. SIGKILL (-9) stays
  uncatchable, so that one case still reparents to init. The watcher does
  NOT respawn the child on edits (it hot-reloads in-process); the respawn
  loop is dormant and kept only for crash detection.

## Dependencies

- clap: CLI argument parsing
- colored: Terminal colors
- notify: File system watcher
- grass: SCSS compiler
- which: Binary lookup
- ctrlc: Signal handling
- tree-sitter + tree-sitter-python / -php / -ruby / -typescript / -rust: real
  per-language AST parsing for the native `tina4 metrics` engine (ADR-0002,
  src/metrics.rs). These grammar crates add ~6MB to the release binary (the C
  parsers); accepted for accurate cross-language complexity/MI. Core frameworks
  stay zero-dep — this is the Rust CLI, and Carbonah already sets the tree-sitter
  precedent in-house.
  `-rust` was added so the engine can measure its own implementation language;
  it cost +1.07MB (8.68MB -> 9.75MB, measured on macOS arm64, release profile).
  There is deliberately NO Pascal/Delphi grammar: the only published crate
  (tree-sitter-pascal 0.10.2) cannot parse Delphi 10.3+ inline loop variables and
  leaves 51.5% of the real tina4delphi corpus unparsed, so `.pas` is not claimed
  rather than reported wrong.

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
