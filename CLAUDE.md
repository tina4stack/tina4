# Tina4 CLI

Version 3.8.37 — Unified CLI for Python, PHP, Ruby, and Node.js Tina4 frameworks.

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
                                 own CLAUDE.md, and opens the tool.
                                 --dry-run = preview only. --skip-install = scaffold, no installs.
tina4 init <language> <path>     Scaffold a new project (python, php, ruby, nodejs, tina4js)
tina4 serve                      Start dev server with file watcher + SCSS + browser open
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
