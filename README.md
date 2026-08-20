# tina4

<p align="center">
  <a href="https://crates.io/crates/tina4"><img src="https://img.shields.io/crates/v/tina4?color=7b1fa2&label=crates.io" alt="crates.io"></a>
  <a href="https://tina4.com"><img src="https://img.shields.io/badge/docs-tina4.com-7b1fa2" alt="Docs"></a>
</p>

Unified CLI for the [Tina4](https://tina4.com) framework: Python, PHP, Ruby, and Node.js.

A single Rust binary that auto-detects your project language, compiles SCSS, watches files for hot-reload, and delegates to the language-specific CLI.

## Install

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

Or with `wget`:

```sh
wget -qO- https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

The script auto-detects your OS and architecture, downloads the correct binary from the latest GitHub release, and installs it to `/usr/local/bin`. Override the install location with:

```sh
TINA4_INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/tina4stack/tina4/main/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\tina4` and automatically adds it to your user PATH. Open a new terminal after installation.

### Windows (package managers)

```powershell
# Scoop (no admin needed)
scoop bucket add tina4 https://github.com/tina4stack/scoop-bucket
scoop install tina4

# Chocolatey (admin shell)
choco install tina4

# winget
winget install Tina4Stack.Tina4
```

Each channel tracks every release automatically. See
[`packaging/README.md`](packaging/README.md) for how the manifests are published.

### Homebrew (macOS / Linux)

```sh
brew install tina4stack/tap/tina4
```

### Debian / Ubuntu (apt)

```sh
curl -fsSL https://apt.tina4.com/tina4.asc | sudo gpg --dearmor -o /usr/share/keyrings/tina4.gpg
echo "deb [signed-by=/usr/share/keyrings/tina4.gpg] https://apt.tina4.com stable main" | sudo tee /etc/apt/sources.list.d/tina4.list
sudo apt-get update && sudo apt-get install tina4
```

Signed metadata, amd64 + arm64, and `apt-get upgrade` keeps it current. Or grab a
single `.deb` from a [release](https://github.com/tina4stack/tina4/releases) and
`sudo apt install ./tina4_<ver>-1_amd64.deb`.

### From source (requires Rust)

```sh
cargo install --git https://github.com/tina4stack/tina4.git
```

### Manual download

Pre-built binaries for every platform are attached to each [GitHub release](https://github.com/tina4stack/tina4/releases):

| Platform | Binary |
|----------|--------|
| macOS ARM64 (Apple Silicon) | `tina4-darwin-arm64` |
| macOS x86_64 (Intel) | `tina4-darwin-amd64` |
| Linux x86_64 | `tina4-linux-amd64` |
| Linux ARM64 | `tina4-linux-arm64` |
| Windows x86_64 | `tina4-windows-amd64.exe` |

## Quick start

```sh
# Check your environment
tina4 doctor

# Create a new project (prompts for language if multiple runtimes are installed)
tina4 init php ./my-app

# Start the dev server with SCSS compilation and hot-reload
cd my-app
tina4 serve
```

## Commands

| Command | Description |
|---------|-------------|
| `tina4 doctor` | Check installed languages, package managers, and Tina4 CLIs |
| `tina4 install <language>` | Install a language runtime (python, php, ruby, nodejs) |
| `tina4 init [language] <path>` | Scaffold a new Tina4 project. Prompts for language if not specified and multiple runtimes are available |
| `tina4 serve [--port N] [--host H] [--dev] [--production] [--no-browser]` | Compile SCSS, start the dev server, watch files, open the browser |
| `tina4 scss [-w]` | Compile SCSS files (`src/scss` → `src/public/css`). Use `-w` to watch |
| `tina4 migrate [--create <name>]` | Run database migrations or create a new one |
| `tina4 test` | Run project tests (delegated to the framework CLI) |
| `tina4 routes` | List registered routes |
| `tina4 generate <type> <name>` | Generate scaffolding: model, route, migration, middleware |
| `tina4 ai [--all] [--force]` | Detect AI coding tools and install framework context/skills |
| `tina4 console` | Start an interactive REPL with the framework loaded (delegated) |
| `tina4 env [--sync] [--example] [--list]` | Scan the project for referenced env vars, merge with `.env.example`, prompt for missing values |
| `tina4 agent [--port N]` | Start the AI agent server for Code With Me |
| `tina4 books` | Download the Tina4 book into the current directory |
| `tina4 docs` | Download framework-specific documentation into `.tina4-docs/` |
| `tina4 i-want-to-stop-using-v2-and-switch-to-v3` | Migrate a v2 project to the v3 structure |
| `tina4 update` / `tina4 upgrade` | Self-update the client and refresh installed Tina4 AI skills |

### AI skills

Use the Tina4 client to install or refresh skills. It shows a menu for Claude,
Codex, Cursor, or all three, and chooses the correct location and current
released skills version for you.

```bash
# Choose interactively (recommended)
tina4 skills
```

For automation, `tina4 skills claude`, `tina4 skills codex`,
`tina4 skills cursor`, and `tina4 skills all` remain available.

`tina4 setup` offers the same choice during first-time setup and writes `AGENTS.md` for Codex projects. Framework repos also ship Cursor entrypoints under `.cursor/skills` (mirroring `.agents/skills` for Codex). The standalone installer remains available for CI and advanced automation.

## How it works

1. **Language detection**: scans for `pyproject.toml`, `composer.json`, `Gemfile`, or `package.json` to determine the project language
2. **SCSS compilation**: uses the [grass](https://github.com/connorskees/grass) crate (pure Rust Sass compiler) so individual frameworks don't need their own SCSS compilers
3. **File watching**: monitors `src/`, `migrations/`, and `.env` for changes. On a meaningful change it POSTs `/__dev/api/reload` to the framework (the server keeps running); the framework then broadcasts the reload to the browser via WebSocket (`/__dev_reload`) with a polling fallback (`GET /__dev/api/mtime`). SCSS changes are recompiled in-place and signalled as `type: "css"` so the browser swaps the stylesheet without a full reload. Events are filtered to real source changes: metadata/access events, `__pycache__`, `.git`, `node_modules`, `vendor`, `dist`, `target`, `logs`, `.log`/`.db*`/`.pyc`/`.swp` files are ignored, and a real mtime check defeats overlayfs / polling-mode spurious events (Podman, distrobox)
4. **Delegation**: forwards commands like `migrate`, `test`, `routes`, `console`, and `ai` to `tina4python`, `tina4php`, `tina4ruby`, or `tina4nodejs` as appropriate
5. **Self-update**: `tina4 update` checks GitHub releases and replaces the binary in-place. Also detects and removes old v2 CLI binaries that may be shadowing the new CLI

## Upgrading from v2

If you have an older v2 Tina4 project, run `tina4 i-want-to-stop-using-v2-and-switch-to-v3` inside the project directory. The verbose name is deliberate: this is a one-way migration and we want you to mean it. It will:

- Move top-level directories (`routes/`, `orm/`, `templates/`, etc.) into `src/`
- Update dependency versions in your manifest file to v3
- Delegate any language-specific code changes to the framework CLI

If you have old v2 CLI binaries (`tina4python`, `tina4php`, etc.) installed globally, `tina4 update` will detect and remove them.

## License

MIT
