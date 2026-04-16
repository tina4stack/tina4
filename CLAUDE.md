# Tina4 CLI

Version 3.8.15 — Unified CLI for Python, PHP, Ruby, and Node.js Tina4 frameworks.

## Build & Test

- Language: Rust (2021 edition)
- Build: `cargo build --release`
- Test: `cargo test`
- Install: `cargo install tina4` or download from crates.io
- Update: `cargo install tina4 --force`

## Commands

```
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
