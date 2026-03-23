pub mod console;
mod detect;
mod doctor;
mod generate;
mod init;
mod install;
mod scss;
mod upgrade;
mod watcher;

use clap::{Parser, Subcommand};
use colored::Colorize;

use crate::console::{icon_eye, icon_fail, icon_info, icon_ok, icon_play, icon_warn};

#[derive(Parser)]
#[command(
    name = "tina4",
    version = "3.3.0",
    about = "Tina4 — Unified CLI for Python, PHP, Ruby, and Node.js",
    long_about = "The Tina4 CLI detects your project language, manages runtimes,\ncompiles SCSS, watches files for dev-reload, and delegates\nto the language-specific CLI (tina4python, tina4php, tina4ruby, tina4nodejs)."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check installed languages and tools
    Doctor,

    /// Install a language runtime (python, php, ruby, nodejs)
    Install {
        /// Language to install: python, php, ruby, nodejs
        lang: String,
    },

    /// Scaffold a new Tina4 project: tina4 init <language> <path>
    Init {
        /// Language: python, php, ruby, nodejs
        lang: Option<String>,
        /// Project directory (absolute or relative path)
        path: Option<String>,
    },

    /// Start the server with file watcher and SCSS compilation.
    /// Production servers are auto-detected; use --dev to force the dev server.
    Serve {
        /// Port number (default: auto per framework — python:7145, php:7146, ruby:7147, nodejs:7148)
        #[arg(short, long)]
        port: Option<u16>,

        /// Host address (default: 0.0.0.0)
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Force dev server even if a production server is available
        #[arg(long)]
        dev: bool,

        /// Install and use the best production server for the detected framework
        #[arg(long)]
        production: bool,
    },

    /// Compile SCSS files from src/scss/ to src/public/css/
    Scss {
        /// Input directory (default: src/scss)
        #[arg(short, long, default_value = "src/scss")]
        input: String,
        /// Output directory (default: src/public/css)
        #[arg(short, long, default_value = "src/public/css")]
        output: String,
        /// Minify output
        #[arg(short, long)]
        minify: bool,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
    },

    /// Run database migrations (delegates to language CLI)
    Migrate {
        /// Create a new migration file with this description
        #[arg(long)]
        create: Option<String>,
    },

    /// Run tests (delegates to language CLI)
    Test,

    /// List registered routes (delegates to language CLI)
    Routes,

    /// Generate scaffolding: model, route, migration, middleware
    Generate {
        /// What to generate: model, route, migration, middleware
        #[arg()]
        what: String,
        /// Name or path
        #[arg()]
        name: String,
    },

    /// Detect AI coding tools and install framework context/skills
    Ai {
        /// Install context for ALL known AI tools (not just detected ones)
        #[arg(long)]
        all: bool,
        /// Overwrite existing context files
        #[arg(long)]
        force: bool,
    },

    /// Upgrade a v2 Tina4 project to v3 structure
    Upgrade,

    /// Self-update the tina4 binary
    Update,

    /// Download the Tina4 book into the current directory
    Books,
}

fn main() {
    console::enable_ansi();
    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => doctor::run(),

        Commands::Install { lang } => install::run(&lang),

        Commands::Init { lang, path } => init::run(lang.as_deref(), path.as_deref()),

        Commands::Serve { port, host, dev, production } => handle_serve(port, &host, dev, production),

        Commands::Scss {
            input,
            output,
            minify,
            watch,
        } => {
            scss::compile_dir(&input, &output, minify);
            if watch {
                println!(
                    "{} Watching {} for SCSS changes...",
                    icon_play().green(),
                    input.cyan()
                );
                watcher::watch_scss(&input, &output, minify);
            }
        }

        Commands::Migrate { create } => {
            delegate_command(if let Some(desc) = create {
                vec!["migrate:create".into(), desc]
            } else {
                vec!["migrate".into()]
            });
        }

        Commands::Test => delegate_command(vec!["test".into()]),

        Commands::Routes => delegate_command(vec!["routes".into()]),

        Commands::Generate { what, name } => generate::run(&what, &name),

        Commands::Ai { all, force } => {
            let mut args = vec!["ai".to_string()];
            if all { args.push("--all".into()); }
            if force { args.push("--force".into()); }
            delegate_command(args);
        }

        Commands::Upgrade => upgrade::run(),

        Commands::Update => handle_update(),

        Commands::Books => handle_books(),
    }
}

// ── Serve ────────────────────────────────────────────────────────

fn handle_serve(port: Option<u16>, host: &str, force_dev: bool, force_production: bool) {
    let lang = detect::detect_language();

    let info = match lang {
        Some(i) => i,
        None => {
            eprintln!(
                "{} No Tina4 project detected. Run: tina4 init <language> <path>",
                icon_fail().red()
            );
            std::process::exit(1);
        }
    };

    // Use framework-specific default port if not overridden
    let port = port.unwrap_or_else(|| info.default_port());

    println!(
        "{} Detected {} project",
        icon_ok().green(),
        info.language.cyan()
    );

    // Set TINA4_DEBUG=true when --dev flag is used, so the framework
    // CLI forces the dev server even if a production server is installed
    if force_dev {
        std::env::set_var("TINA4_DEBUG", "true");
        println!(
            "{} Dev mode forced — production server detection disabled",
            icon_info().blue()
        );
    }

    // --production: install best production server if not available, force debug off
    if force_production {
        std::env::set_var("TINA4_DEBUG", "false");
        println!(
            "{} Production mode — installing best server if needed",
            icon_play().green()
        );
        install_production_server(&info);
    }

    // Compile SCSS
    let scss_dir = "src/scss";
    let css_dir = "src/public/css";
    if std::path::Path::new(scss_dir).exists() {
        scss::compile_dir(scss_dir, css_dir, false);
    }

    // Start language server (auto-detects production server internally)
    let cli = info.cli_name();
    println!(
        "{} Starting {} on {}:{}",
        icon_play().green(),
        cli.cyan(),
        host.yellow(),
        port.to_string().yellow()
    );

    let mut server = match start_language_server(&info, port, host) {
        Some(child) => child,
        None => {
            eprintln!("{} Failed to start server", icon_fail().red());
            std::process::exit(1);
        }
    };

    // File watcher (blocks)
    println!(
        "{} File watcher active — src/, migrations/, .env",
        icon_eye().green()
    );
    watcher::watch_and_reload(scss_dir, css_dir, &info, port, host, &mut server);
}

fn install_production_server(info: &detect::ProjectInfo) {
    let (name, check_fn, install_cmd): (&str, Box<dyn Fn() -> bool>, &str) = match info.language.as_str() {
        "python" => ("uvicorn", Box::new(|| which::which("uvicorn").is_ok()), "uv add uvicorn"),
        "php" => ("opcache", Box::new(|| true), ""), // built-in
        "ruby" => ("puma", Box::new(|| {
            console::shell_output("gem list puma")
                .map(|o| !o.stdout.is_empty() && String::from_utf8_lossy(&o.stdout).contains("puma"))
                .unwrap_or(false)
        }), "gem install puma --no-doc"),
        "nodejs" => ("cluster", Box::new(|| true), ""), // built-in
        _ => return,
    };

    if check_fn() {
        println!("  {} {} already installed", icon_ok().green(), name.cyan());
        return;
    }

    println!(
        "  {} Installing {}...",
        icon_play().green(),
        name.cyan()
    );
    match console::shell_exec(install_cmd) {
        Ok(s) if s.success() => println!("  {} {} installed", icon_ok().green(), name.cyan()),
        _ => println!("  {} Failed to install {} — using dev server", icon_warn().yellow(), name),
    }
}

fn start_language_server(
    info: &detect::ProjectInfo,
    port: u16,
    host: &str,
) -> Option<std::process::Child> {
    let port_s = port.to_string();

    let result = match info.language.as_str() {
        "python" => {
            // Use uv run if .venv exists, otherwise python directly
            if std::path::Path::new(".venv").exists() {
                std::process::Command::new("uv")
                    .args(["run", "python", "app.py"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
            } else {
                std::process::Command::new(console::python_cmd())
                    .args(["app.py"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
            }
        }
        "php" => {
            // Check vendor/ exists before trying to serve
            if !std::path::Path::new("vendor").exists() {
                eprintln!(
                    "{} Dependencies not installed. Run: {}",
                    icon_fail().red(),
                    "composer install".cyan()
                );
                return None;
            }
            let addr = format!("{}:{}", host, port);
            let (cmd, mut cmd_args) = resolve_cli(info);
            cmd_args.extend(["serve".into(), addr]);
            std::process::Command::new(&cmd)
                .args(&cmd_args)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
        }
        "ruby" => {
            // Use bundle exec if Gemfile exists
            if std::path::Path::new("Gemfile").exists() {
                std::process::Command::new("bundle")
                    .args(["exec", "ruby", "app.rb"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
            } else {
                std::process::Command::new("ruby")
                    .args(["app.rb"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
            }
        }
        "nodejs" => {
            // Use npx tsx for TypeScript
            let entry = if std::path::Path::new("app.ts").exists() { "app.ts" } else { "app.js" };
            std::process::Command::new("npx")
                .args(["tsx", entry])
                .env("PORT", &port_s)
                .env("HOST", host)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
        }
        _ => return None,
    };

    result.ok()
}

// ── Delegate ─────────────────────────────────────────────────────

/// Resolve the language CLI command and arguments for the detected project.
/// PHP needs special handling: `php vendor/bin/tina4php` instead of bare `tina4php`.
fn resolve_cli(info: &detect::ProjectInfo) -> (String, Vec<String>) {
    match info.language.as_str() {
        "php" => {
            let vendor_path = console::php_vendor_bin("tina4php");
            let cli_path = if std::path::Path::new(&vendor_path).exists() {
                vendor_path
            } else if std::path::Path::new("bin/tina4php").exists() {
                "bin/tina4php".to_string()
            } else {
                // Fallback: try global tina4php
                return ("tina4php".into(), vec![]);
            };
            ("php".into(), vec![cli_path])
        }
        _ => (info.cli_name().into(), vec![]),
    }
}

fn delegate_command(args: Vec<String>) {
    match detect::detect_language() {
        Some(info) => {
            // For PHP, check vendor/ exists
            if info.language == "php" && !std::path::Path::new("vendor").exists() {
                eprintln!(
                    "{} Dependencies not installed. Run: {}",
                    icon_fail().red(),
                    "composer install".cyan()
                );
                std::process::exit(1);
            }

            let (cmd, mut cmd_args) = resolve_cli(&info);
            cmd_args.extend(args);

            match std::process::Command::new(&cmd).args(&cmd_args).status() {
                Ok(s) if !s.success() => std::process::exit(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("{} Failed to run {} {}: {}", icon_fail().red(), cmd, cmd_args.join(" "), e);
                    std::process::exit(1);
                }
                _ => {}
            }
        }
        None => {
            eprintln!(
                "{} No Tina4 project detected in current directory",
                icon_fail().red()
            );
            std::process::exit(1);
        }
    }
}

// ── Update ───────────────────────────────────────────────────────

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "tina4stack/tina4";
const BOOK_REPO: &str = "tina4stack/tina4-book";

fn handle_books() {
    let dest = std::path::Path::new("tina4-book");

    if dest.exists() {
        eprintln!(
            "{} A {} directory already exists. Remove it first if you want a fresh copy.",
            icon_warn().yellow(),
            "tina4-book/".cyan()
        );
        return;
    }

    let zip_url = format!(
        "https://github.com/{}/archive/refs/heads/main.zip",
        BOOK_REPO
    );
    let zip_path = std::path::PathBuf::from("tina4-book.zip");

    println!(
        "{} Downloading Tina4 book...",
        icon_play().green()
    );

    if !download_file(&zip_url, &zip_path) {
        eprintln!(
            "{} Download failed. Check your connection or visit:\n  https://github.com/{}",
            icon_fail().red(),
            BOOK_REPO
        );
        return;
    }

    // Extract the zip
    println!("{} Extracting...", icon_play().green());

    let extracted = if console::is_windows() {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '.' -Force",
                    zip_path.display()
                ),
            ])
            .status()
    } else {
        std::process::Command::new("unzip")
            .args(["-qo", &zip_path.to_string_lossy(), "-d", "."])
            .status()
    };

    if !matches!(extracted, Ok(s) if s.success()) {
        eprintln!("{} Failed to extract archive", icon_fail().red());
        std::fs::remove_file(&zip_path).ok();
        return;
    }

    // Rename extracted folder to tina4-book/
    let extracted_dir = std::path::Path::new("tina4-book-main");
    if extracted_dir.exists() {
        if std::fs::rename(extracted_dir, dest).is_err() {
            eprintln!(
                "{} Could not rename {} to {}",
                icon_fail().red(),
                "tina4-book-main".dimmed(),
                "tina4-book/".cyan()
            );
        }
    }

    // Clean up zip
    std::fs::remove_file(&zip_path).ok();

    println!(
        "{} Tina4 book downloaded to {}",
        icon_ok().green(),
        "tina4-book/".cyan()
    );
}

fn handle_update() {
    println!("{} Checking for updates...", icon_play().green());

    // Step 1: Check for and clean up old v2 CLI binaries
    clean_v2_binaries();

    // Step 2: Get latest version from GitHub API
    let latest_tag = match get_latest_version() {
        Some(tag) => tag,
        None => {
            eprintln!(
                "{} Could not check latest version. Download manually from:\n  https://github.com/{}/releases",
                icon_warn().yellow(), REPO
            );
            return;
        }
    };

    let latest_ver = latest_tag.trim_start_matches('v');
    println!(
        "  {} Current: {}  Latest: {}",
        icon_info().blue(),
        CURRENT_VERSION.cyan(),
        latest_ver.cyan()
    );

    if latest_ver == CURRENT_VERSION {
        println!("{} Already up to date", icon_ok().green());
        return;
    }

    // Step 3: Download and replace binary
    let binary_name = get_binary_name();
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, latest_tag, binary_name
    );

    println!(
        "{} Downloading {} ...",
        icon_play().green(),
        binary_name.cyan()
    );

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Cannot determine current executable path: {}", icon_fail().red(), e);
            return;
        }
    };

    let tmp_path = current_exe.with_extension("tmp");

    if !download_file(&download_url, &tmp_path) {
        eprintln!(
            "{} Download failed. Download manually from:\n  https://github.com/{}/releases",
            icon_fail().red(), REPO
        );
        return;
    }

    // Replace current binary
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&tmp_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&tmp_path, perms).ok();
        }
    }

    let backup_path = current_exe.with_extension("old");
    // On Windows, can't replace running exe directly — rename current first
    if std::fs::rename(&current_exe, &backup_path).is_err() {
        // Try copy instead
        if std::fs::copy(&current_exe, &backup_path).is_err() {
            eprintln!("{} Cannot backup current binary", icon_fail().red());
            std::fs::remove_file(&tmp_path).ok();
            return;
        }
    }

    if std::fs::rename(&tmp_path, &current_exe).is_err() {
        if std::fs::copy(&tmp_path, &current_exe).is_err() {
            eprintln!("{} Cannot replace binary — restoring backup", icon_fail().red());
            std::fs::rename(&backup_path, &current_exe).ok();
            std::fs::remove_file(&tmp_path).ok();
            return;
        }
        std::fs::remove_file(&tmp_path).ok();
    }

    // Clean up backup
    std::fs::remove_file(&backup_path).ok();

    println!(
        "{} Updated tina4 {} → {}",
        icon_ok().green(),
        CURRENT_VERSION.dimmed(),
        latest_ver.cyan()
    );
}

/// Detect and remove old v2 CLI binaries that may shadow the v3 CLI.
fn clean_v2_binaries() {
    let stale_names = ["tina4python", "tina4php", "tina4ruby", "tina4nodejs"];
    let mut found_any = false;

    for name in &stale_names {
        if let Ok(path) = which::which(name) {
            // Check if it's a global binary (not in vendor/bin or .venv)
            let path_str = path.to_string_lossy();
            if path_str.contains("vendor") || path_str.contains(".venv") || path_str.contains("node_modules") {
                continue;
            }

            // Try to detect if it's v2 by running --version
            let is_v2 = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .map(|o| {
                    let out = String::from_utf8_lossy(&o.stdout).to_string()
                        + &String::from_utf8_lossy(&o.stderr);
                    // v2 indicators: Thor, old version numbers, deprecation warnings
                    out.contains("Thor") || out.contains("Deprecation") || out.contains("1.") || out.contains("2.")
                })
                .unwrap_or(false);

            if is_v2 {
                if !found_any {
                    println!(
                        "\n{} Found old v2 CLI binaries on PATH:",
                        icon_warn().yellow()
                    );
                    found_any = true;
                }
                println!("  {} {} ({})", icon_fail().red(), name, path_str.dimmed());

                // Remove it
                match std::fs::remove_file(&path) {
                    Ok(_) => println!("    {} Removed", icon_ok().green()),
                    Err(_) => {
                        // Try with .bat extension on Windows
                        let bat_path = path.with_extension("bat");
                        std::fs::remove_file(&bat_path).ok();
                        println!(
                            "    {} Cannot remove — delete manually: {}",
                            icon_warn().yellow(),
                            path_str
                        );
                    }
                }
            }
        }
    }

    // Also check for old non-Rust tina4 binaries
    if let Ok(tina4_path) = which::which("tina4") {
        let current_exe = std::env::current_exe().unwrap_or_default();
        if tina4_path != current_exe {
            // There's another tina4 on PATH that isn't us
            let is_old = std::process::Command::new(&tina4_path)
                .arg("--version")
                .output()
                .map(|o| {
                    let out = String::from_utf8_lossy(&o.stdout).to_string()
                        + &String::from_utf8_lossy(&o.stderr);
                    out.contains("Thor") || out.contains("Deprecation") || !out.contains("tina4")
                })
                .unwrap_or(false);

            if is_old {
                if !found_any {
                    println!(
                        "\n{} Found old v2 CLI binaries on PATH:",
                        icon_warn().yellow()
                    );
                }
                let path_str = tina4_path.to_string_lossy();
                println!("  {} tina4 ({})", icon_fail().red(), path_str.dimmed());
                match std::fs::remove_file(&tina4_path) {
                    Ok(_) => {
                        // Also remove .bat wrapper if present
                        let bat = tina4_path.with_extension("bat");
                        std::fs::remove_file(bat).ok();
                        println!("    {} Removed", icon_ok().green());
                    }
                    Err(_) => println!(
                        "    {} Cannot remove — delete manually: {}",
                        icon_warn().yellow(),
                        path_str
                    ),
                }
            }
        }
    }

    if found_any {
        println!();
    }
}

fn get_latest_version() -> Option<String> {
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", REPO);

    let output = if console::is_windows() {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command",
                &format!("(Invoke-RestMethod -Uri '{}' -Headers @{{'User-Agent'='tina4-cli'}}).tag_name", api_url)])
            .output()
            .ok()?
    } else {
        std::process::Command::new("curl")
            .args(["-fsSL", "-H", "User-Agent: tina4-cli", &api_url])
            .output()
            .ok()?
    };

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if console::is_windows() {
        // PowerShell returns the tag directly
        if text.starts_with('v') {
            return Some(text);
        }
    } else {
        // curl returns JSON, extract tag_name
        for line in text.lines() {
            if line.contains("\"tag_name\"") {
                let tag = line.split('"').nth(3)?;
                return Some(tag.to_string());
            }
        }
    }

    None
}

fn get_binary_name() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };

    if cfg!(target_os = "windows") {
        format!("tina4-{}-{}.exe", os, arch)
    } else {
        format!("tina4-{}-{}", os, arch)
    }
}

fn download_file(url: &str, dest: &std::path::Path) -> bool {
    let dest_str = dest.to_string_lossy();

    let status = if console::is_windows() {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command",
                &format!("Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing", url, dest_str)])
            .status()
    } else {
        std::process::Command::new("curl")
            .args(["-fsSL", "-o", &dest_str, url])
            .status()
    };

    matches!(status, Ok(s) if s.success())
}
