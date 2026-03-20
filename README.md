# tina4

Unified CLI for the [Tina4](https://tina4.com) framework — Python, PHP, Ruby, and Node.js.

A single Rust binary that auto-detects your project language, compiles SCSS, watches files for hot-reload, and delegates to the language-specific CLI.

## Install

### macOS / Linux (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

Or with `wget`:

```sh
wget -qO- https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

The script auto-detects your OS and architecture, downloads the correct binary from the latest GitHub release, and installs it to `/usr/local/bin`. You can override the install location:

```sh
TINA4_INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/tina4stack/tina4/main/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\tina4` and automatically adds it to your user PATH. Open a new terminal after installation.

### Homebrew (macOS / Linux)

```sh
brew install tina4stack/tap/tina4
```

### From source (requires Rust)

```sh
cargo install --git https://github.com/tina4stack/tina4.git
```

### Manual download

Pre-built binaries for every platform are attached to each [GitHub release](https://github.com/tina4stack/tina4/releases). Download the binary for your platform, make it executable, and place it on your PATH:

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

# Install a language runtime and its Tina4 CLI
tina4 install python

# Create a new project
tina4 init

# Start the dev server with SCSS compilation and hot-reload
tina4 serve
```

## Commands

| Command | Description |
|---------|-------------|
| `tina4 doctor` | Check installed languages, package managers, and Tina4 CLIs |
| `tina4 install <language\|all>` | Install a language runtime and its Tina4 CLI |
| `tina4 init` | Scaffold a new Tina4 project |
| `tina4 serve` | Compile SCSS, start the dev server, and watch for changes |
| `tina4 scss` | Compile SCSS files (src/scss → src/public/css) |
| `tina4 migrate` | Run database migrations |
| `tina4 test` | Run project tests |
| `tina4 routes` | List registered routes |
| `tina4 update` | Update the Tina4 CLI |

## How it works

1. **Language detection** — scans for `pyproject.toml`, `composer.json`, `Gemfile`, or `package.json` to determine the project language
2. **SCSS compilation** — uses the [grass](https://github.com/connorskees/grass) crate (pure Rust Sass compiler) so individual frameworks don't need their own SCSS compilers
3. **File watching** — monitors `src/`, `migrations/`, and `.env` for changes; recompiles SCSS and restarts the dev server automatically
4. **Delegation** — forwards commands to `tina4python`, `tina4php`, `tina4ruby`, or `tina4nodejs` as appropriate

## License

MIT
