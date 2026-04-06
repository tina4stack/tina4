pub mod console;
mod agent;
mod env_config;
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
    version = env!("CARGO_PKG_VERSION"),
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

        /// Do not open the browser on startup
        #[arg(long)]
        no_browser: bool,
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

    /// Stop using v2 and switch your Tina4 project to v3 structure
    #[command(name = "i-want-to-stop-using-v2-and-switch-to-v3")]
    IWantToStopUsingV2AndSwitchToV3,

    /// Self-update the tina4 binary
    Update,

    /// Download the Tina4 book into the current directory
    Books,

    /// Download framework-specific documentation into .tina4-docs/
    Docs,

    /// Start an interactive REPL with the framework loaded
    Console,

    /// Start the AI agent server for Code With Me
    Agent {
        /// Port number (default: framework port + 2000)
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Configure environment variables interactively
    Env {
        /// Just scan and sync — don't prompt interactively
        #[arg(long)]
        sync: bool,
        /// Only generate .env.example
        #[arg(long)]
        example: bool,
        /// List all env vars the project uses
        #[arg(long)]
        list: bool,
    },
}

fn main() {
    console::enable_ansi();
    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => doctor::run(),

        Commands::Install { lang } => install::run(&lang),

        Commands::Init { lang, path } => init::run(lang.as_deref(), path.as_deref()),

        Commands::Serve { port, host, dev, production, no_browser } => handle_serve(port, &host, dev, production, no_browser),

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

        Commands::Agent { port } => {
            let default_port = 9145u16; // default agent port
            agent::run(port.unwrap_or(default_port));
        }

        Commands::Ai { all, force } => {
            // Check if this is a tina4-js (frontend) project — handle directly
            if is_tina4js_project() {
                handle_tina4js_ai(all, force);
            } else {
                let mut args = vec!["ai".to_string()];
                if all { args.push("--all".into()); }
                if force { args.push("--force".into()); }
                delegate_command(args);
            }
        }

        Commands::IWantToStopUsingV2AndSwitchToV3 => upgrade::run(),

        Commands::Update => handle_update(),

        Commands::Console => delegate_command(vec!["console".into()]),
        Commands::Books => handle_books(),
        Commands::Docs => handle_docs(),
        Commands::Env { sync, example, list } => env_config::run(sync, example, list),
    }
}

// ── Serve ────────────────────────────────────────────────────────

pub fn handle_serve(port: Option<u16>, host: &str, force_dev: bool, force_production: bool, no_browser: bool) {
    // Background version check — warns if CLI is outdated
    std::thread::spawn(|| {
        if let Some(latest_tag) = get_latest_version() {
            let latest = latest_tag.trim_start_matches('v');
            if latest != CURRENT_VERSION {
                eprintln!(
                    "\n{} Tina4 CLI {} available (you have {}). Run: tina4 update\n",
                    icon_warn().yellow(),
                    latest.cyan(),
                    CURRENT_VERSION.dimmed()
                );
            }
        }
    });

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

    // Port priority: CLI flag > TINA4_PORT env/dotenv > PORT env/dotenv > framework default
    let requested_port = port.unwrap_or_else(|| {
        // Read .env file if it exists (don't override existing env vars)
        if let Ok(contents) = std::fs::read_to_string(".env") {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    if std::env::var(key).is_err() {
                        std::env::set_var(key, value);
                    }
                }
            }
        }
        // Check TINA4_PORT first, then PORT, then framework default
        std::env::var("TINA4_PORT")
            .or_else(|_| std::env::var("PORT"))
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or_else(|| info.default_port())
    });

    // If --port was explicitly provided, kill whatever is on that port.
    // Otherwise, auto-increment to find a free port.
    let explicit_port = port.is_some();
    let port = if explicit_port {
        if std::net::TcpListener::bind(("127.0.0.1", requested_port)).is_err() {
            println!(
                "{} Port {} in use — killing existing process...",
                icon_warn().yellow(),
                requested_port.to_string().cyan()
            );
            if console::kill_port(requested_port) {
                println!(
                    "{} Port {} freed",
                    icon_ok().green(),
                    requested_port.to_string().cyan()
                );
            } else {
                eprintln!(
                    "{} Could not free port {} — process may require manual termination",
                    icon_fail().red(),
                    requested_port
                );
                std::process::exit(1);
            }
        }
        requested_port
    } else {
        // Default port: kill whatever is on it and take it over
        if !std::net::TcpListener::bind(("127.0.0.1", requested_port)).is_ok() {
            println!(
                "{} Port {} in use — killing existing process...",
                icon_warn().yellow(),
                requested_port.to_string().cyan()
            );
            if console::kill_port(requested_port) {
                println!(
                    "{} Port {} freed",
                    icon_ok().green(),
                    requested_port.to_string().cyan()
                );
            } else {
                eprintln!(
                    "{} Could not free port {} — process may require manual termination",
                    icon_fail().red(),
                    requested_port
                );
                std::process::exit(1);
            }
        }
        requested_port
    };

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

    // Give the server a moment to bind, then open browser (unless --no-browser)
    std::thread::sleep(std::time::Duration::from_secs(2));
    let url = format!("http://localhost:{}", port);
    if no_browser {
        println!("{} Server ready: {}", icon_ok().green(), url.cyan());
    } else {
        console::open_browser(&url);
        println!("{} Browser opened: {}", icon_ok().green(), url.cyan());
    }

    // File watcher (blocks) — skip for tina4js since Vite has its own HMR
    if info.language == "tina4js" {
        println!(
            "{} Vite HMR active — press Ctrl+C to stop",
            icon_eye().green()
        );
        // Block until server exits
        let _ = server.wait();
    } else {
        println!(
            "{} File watcher active — src/, migrations/, .env",
            icon_eye().green()
        );
        watcher::watch_and_reload(scss_dir, css_dir, &info, port, host, &mut server);
    }
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
        "tina4js" => ("vite", Box::new(|| true), ""), // uses vite build + preview
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

/// Apply pre_exec to put the child in its own process group on Unix,
/// so we can kill the entire group on restart (prevents EADDRINUSE).
#[cfg(unix)]
fn set_process_group(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    cmd
}

#[cfg(not(unix))]
fn set_process_group(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
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
                let mut cmd = std::process::Command::new("uv");
                cmd.args(["run", "python", "app.py"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            } else {
                let mut cmd = std::process::Command::new(console::python_cmd());
                cmd.args(["app.py"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
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
            let (cmd_name, mut cmd_args) = resolve_cli(info);
            cmd_args.extend(["serve".into(), addr]);
            let mut cmd = std::process::Command::new(&cmd_name);
            cmd.args(&cmd_args)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            set_process_group(&mut cmd).spawn()
        }
        "ruby" => {
            // Use bundle exec if Gemfile exists
            if std::path::Path::new("Gemfile").exists() {
                // Check that bundle has been installed
                if !std::path::Path::new("Gemfile.lock").exists() {
                    eprintln!(
                        "{} Dependencies not installed. Run: {}",
                        icon_fail().red(),
                        "bundle install".cyan()
                    );
                    return None;
                }
                let mut cmd = std::process::Command::new(console::resolve_cmd("bundle"));
                cmd.args(["exec", "ruby", "app.rb"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            } else {
                let mut cmd = std::process::Command::new("ruby");
                cmd.args(["app.rb"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            }
        }
        "nodejs" => {
            // Check node_modules/ exists before trying to serve
            if !std::path::Path::new("node_modules").exists() {
                eprintln!(
                    "{} Dependencies not installed. Run: {}",
                    icon_fail().red(),
                    "npm install".cyan()
                );
                return None;
            }
            // Use npx tsx for TypeScript (tsx also handles plain .js)
            let entry = if std::path::Path::new("app.ts").exists() { "app.ts" } else { "app.js" };
            let mut cmd = std::process::Command::new(console::resolve_cmd("npx"));
            cmd.args(["tsx", entry])
                .env("PORT", &port_s)
                .env("HOST", host)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            set_process_group(&mut cmd).spawn()
        }
        "tina4js" => {
            // tina4js uses Vite dev server
            if !std::path::Path::new("node_modules").exists() {
                eprintln!(
                    "{} Dependencies not installed. Run: {}",
                    icon_fail().red(),
                    "npm install".cyan()
                );
                return None;
            }
            let mut cmd = std::process::Command::new("npx");
            cmd.args(["vite", "--port", &port_s, "--host", host, "--strictPort"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            set_process_group(&mut cmd).spawn()
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
        "python" => {
            // uv projects: run via 'uv run tina4python' so the venv CLI is found
            if std::path::Path::new("uv.lock").exists() || std::path::Path::new("pyproject.toml").exists() {
                if which::which("uv").is_ok() {
                    return ("uv".into(), vec!["run".into(), "tina4python".into()]);
                }
            }
            // Fallback: try global tina4python or .venv/Scripts
            let venv_cli = if cfg!(windows) { ".venv/Scripts/tina4python.exe" } else { ".venv/bin/tina4python" };
            if std::path::Path::new(venv_cli).exists() {
                (venv_cli.into(), vec![])
            } else {
                ("tina4python".into(), vec![])
            }
        }
        "ruby" => {
            // bundler projects: run via 'bundle exec tina4ruby'
            if std::path::Path::new("Gemfile.lock").exists() {
                if which::which("bundle").is_ok() {
                    return ("bundle".into(), vec!["exec".into(), "tina4ruby".into()]);
                }
            }
            ("tina4ruby".into(), vec![])
        }
        "nodejs" => {
            // npm projects: run via 'npx tina4nodejs'
            if std::path::Path::new("node_modules").exists() {
                if which::which("npx").is_ok() {
                    return ("npx".into(), vec!["tina4nodejs".into()]);
                }
            }
            ("tina4nodejs".into(), vec![])
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

fn handle_docs() {
    let info = match detect::detect_language() {
        Some(i) => i,
        None => {
            eprintln!(
                "{} No Tina4 project detected. Run {} in a Tina4 project directory.",
                icon_fail().red(),
                "tina4 docs".cyan()
            );
            std::process::exit(1);
        }
    };

    let book_dir = match info.language.as_str() {
        "python" => "book-1-python",
        "php" => "book-2-php",
        "ruby" => "book-3-ruby",
        "nodejs" => "book-4-nodejs",
        "tina4js" => "book-5-javascript",
        _ => {
            eprintln!("{} Unsupported language: {}", icon_fail().red(), info.language);
            return;
        }
    };

    let dest = std::path::Path::new(".tina4-docs");
    if dest.exists() {
        // Remove old docs and re-download
        std::fs::remove_dir_all(dest).ok();
    }

    let zip_url = format!(
        "https://github.com/{}/archive/refs/heads/main.zip",
        BOOK_REPO
    );
    let zip_path = std::path::PathBuf::from(".tina4-docs.zip");

    println!(
        "{} Downloading {} documentation...",
        icon_play().green(),
        info.language.cyan()
    );

    if !download_file(&zip_url, &zip_path) {
        eprintln!("{} Download failed.", icon_fail().red());
        return;
    }

    // Extract to temp dir
    let tmp_dir = std::path::Path::new(".tina4-docs-tmp");
    if tmp_dir.exists() {
        std::fs::remove_dir_all(tmp_dir).ok();
    }

    let extracted = if console::is_windows() {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile", "-Command",
                &format!("Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    zip_path.display(), tmp_dir.display()),
            ])
            .status()
    } else {
        std::process::Command::new("unzip")
            .args(["-qo", &zip_path.to_string_lossy(), "-d", &tmp_dir.to_string_lossy()])
            .status()
    };

    if !matches!(extracted, Ok(s) if s.success()) {
        eprintln!("{} Failed to extract archive", icon_fail().red());
        std::fs::remove_file(&zip_path).ok();
        std::fs::remove_dir_all(tmp_dir).ok();
        return;
    }

    // Copy just the relevant book's chapters to .tina4-docs/
    let chapters_src = tmp_dir.join("tina4-book-main").join(book_dir).join("chapters");
    if chapters_src.exists() {
        std::fs::create_dir_all(dest).ok();
        if let Ok(entries) = std::fs::read_dir(&chapters_src) {
            for entry in entries.flatten() {
                let src_path = entry.path();
                let dest_path = dest.join(entry.file_name());
                std::fs::copy(&src_path, &dest_path).ok();
            }
        }
    } else {
        eprintln!("{} Book chapters not found for {}", icon_warn().yellow(), info.language);
    }

    // Clean up
    std::fs::remove_file(&zip_path).ok();
    std::fs::remove_dir_all(tmp_dir).ok();

    // Count files
    let count = std::fs::read_dir(dest)
        .map(|entries| entries.count())
        .unwrap_or(0);

    println!(
        "{} {} docs downloaded to {} ({} chapters)",
        icon_ok().green(),
        info.language.cyan(),
        ".tina4-docs/".cyan(),
        count.to_string().cyan()
    );
    println!(
        "  {} Available in dev overlay at {}",
        icon_info().blue(),
        "/__dev → Docs".cyan()
    );
}

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
    if extracted_dir.exists() && std::fs::rename(extracted_dir, dest).is_err() {
        eprintln!(
            "{} Could not rename {} to {}",
            icon_fail().red(),
            "tina4-book-main".dimmed(),
            "tina4-book/".cyan()
        );
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
        println!("{} CLI already up to date", icon_ok().green());
        // Still check for framework package updates even if CLI is current
        update_framework_package();
        return;
    }

    // Step 3: Download and replace binary — try multiple name variants
    let candidates = get_binary_name_candidates();

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Cannot determine current executable path: {}", icon_fail().red(), e);
            return;
        }
    };

    let tmp_path = current_exe.with_extension("tmp");
    let mut downloaded = false;

    for name in &candidates {
        let url = format!(
            "https://github.com/{}/releases/download/{}/{}",
            REPO, latest_tag, name
        );
        println!(
            "{} Trying {} ...",
            icon_play().green(),
            name.cyan()
        );
        if download_file(&url, &tmp_path) {
            downloaded = true;
            break;
        }
    }

    if !downloaded {
        eprintln!(
            "{} Download failed (tried: {}). Download manually from:\n  https://github.com/{}/releases",
            icon_fail().red(), candidates.join(", "), REPO
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
        "{} Updated tina4 CLI {} → {}",
        icon_ok().green(),
        CURRENT_VERSION.dimmed(),
        latest_ver.cyan()
    );

    // Also update the framework package in the current project
    update_framework_package();
}

/// Ask the user if they want to update the framework package too.
fn update_framework_package() {
    use std::io::Write;

    let info = match detect::detect_language() {
        Some(i) => i,
        None => return, // Not in a project directory — skip
    };

    let lang = info.language.as_str();

    let pkg: &str = match lang {
        "python" => "tina4-python",
        "php" => "tina4stack/tina4php",
        "ruby" => "tina4ruby",
        "nodejs" => "tina4-nodejs",
        _ => return,
    };

    println!();
    print!(
        "  Also update {} framework package? [Y/n]: ",
        pkg.cyan()
    );
    std::io::stdout().flush().ok();

    let mut input = String::new();
    let should_update = match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => false,
        _ => {
            let trimmed = input.trim().to_lowercase();
            trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
        }
    };

    if !should_update {
        let hint = match lang {
            "python" => "uv lock --upgrade-package tina4-python && uv sync".to_string(),
            "php" => "composer update tina4stack/tina4php".to_string(),
            "ruby" => "bundle update tina4ruby".to_string(),
            "nodejs" => "npm update tina4-nodejs".to_string(),
            _ => return,
        };
        println!("  Skipped. To update later: {}", hint);
        return;
    }

    println!(
        "{} Updating {}...",
        icon_play().green(),
        pkg.cyan()
    );

    let success = match lang {
        "python" => {
            // Python needs two steps: update lockfile then sync
            let lock_ok = std::process::Command::new("uv")
                .args(["lock", "--upgrade-package", "tina4-python"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if lock_ok {
                std::process::Command::new("uv")
                    .args(["sync"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        "php" => std::process::Command::new("composer")
            .args(["update", "tina4stack/tina4php"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        "ruby" => std::process::Command::new("bundle")
            .args(["update", "tina4ruby"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        "nodejs" => std::process::Command::new("npm")
            .args(["update", "tina4-nodejs"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        _ => false,
    };

    if success {
        println!("{} {} updated", icon_ok().green(), pkg.cyan());
    } else {
        eprintln!(
            "{} Framework update failed. Check the output above for details.",
            icon_warn().yellow(),
        );
    }
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
            .args(["-fsSL", "-H", "User-Agent: tina4-cli", "-H", "Accept: application/vnd.github+json", &api_url])
            .output()
            .ok()?
    };

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if text.is_empty() {
        return None;
    }

    if console::is_windows() {
        // PowerShell returns the tag directly
        if text.starts_with('v') {
            return Some(text);
        }
    }

    // Parse JSON — find "tag_name": "vX.Y.Z" anywhere in the response
    // Handle both pretty-printed and minified JSON
    if let Some(pos) = text.find("\"tag_name\"") {
        let after = &text[pos..];
        // Find the value after the colon: "tag_name": "v3.3.3"
        let mut in_value = false;
        let mut start = 0;
        for (i, ch) in after.char_indices() {
            if ch == ':' && !in_value {
                in_value = true;
                continue;
            }
            if in_value && ch == '"' && start == 0 {
                start = i + 1;
                continue;
            }
            if in_value && ch == '"' && start > 0 {
                return Some(after[start..i].to_string());
            }
        }
    }

    None
}

/// Return a list of possible binary names to try, handling naming
/// variations across releases (amd64 vs x86_64, darwin vs macos).
fn get_binary_name_candidates() -> Vec<String> {
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };

    let os_variants: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["darwin", "macos"]
    } else if cfg!(target_os = "windows") {
        vec!["windows"]
    } else {
        vec!["linux"]
    };

    let arch_variants: Vec<&str> = if cfg!(target_arch = "aarch64") {
        vec!["arm64", "aarch64"]
    } else {
        vec!["amd64", "x86_64"]
    };

    let mut names = Vec::new();
    for os in &os_variants {
        for arch in &arch_variants {
            names.push(format!("tina4-{}-{}{}", os, arch, ext));
        }
    }
    names
}

fn download_file(url: &str, dest: &std::path::Path) -> bool {
    let dest_str = dest.to_string_lossy();

    let status = if console::is_windows() {
        // Use curl.exe (ships with Windows 10+) for reliable GitHub downloads.
        // PowerShell's Invoke-WebRequest struggles with GitHub's TLS/redirect chain.
        let curl_path = "C:\\Windows\\System32\\curl.exe";
        if std::path::Path::new(curl_path).exists() {
            std::process::Command::new(curl_path)
                .args(["-fsSL", "-o", &dest_str, url])
                .status()
        } else {
            // Fallback to PowerShell with TLS 1.2 forced
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command",
                    &format!("[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing", url, dest_str)])
                .status()
        }
    } else {
        std::process::Command::new("curl")
            .args(["-fsSL", "-o", &dest_str, url])
            .status()
    };

    matches!(status, Ok(s) if s.success())
}

// ── Tina4-js AI context ─────────────────────────────────────────

/// Check if the current directory is a tina4-js (frontend) project.
fn is_tina4js_project() -> bool {
    if let Ok(content) = std::fs::read_to_string("package.json") {
        // Has tina4js dependency but no app.ts (not a Node.js backend project)
        content.contains("\"tina4js\"") && !std::path::Path::new("app.ts").exists()
    } else {
        false
    }
}

/// Handle `tina4 ai` for tina4-js projects — install the tina4-js skill directly.
fn handle_tina4js_ai(_all: bool, force: bool) {
    use std::fs;
    use std::path::Path;

    println!("  {} Detected tina4-js (frontend) project", icon_info());

    // Install CLAUDE.md with tina4-js context
    let claude_path = Path::new("CLAUDE.md");
    if claude_path.exists() && !force {
        println!("  {} CLAUDE.md already exists (use --force to overwrite)", icon_warn());
    } else {
        let content = r#"# Tina4-js Project

Frontend project using tina4-js — the sub-3KB reactive framework.

## Build & Dev

- Install: `npm install`
- Dev: `npm run dev`
- Build: `npm run build`

## Tina4-js Features

- Signals for reactive state
- HTML tagged templates
- Tina4Element for web components
- Built-in routing (hash and history mode)
- WebSocket client with auto-reconnect
- API client with auth headers
- Zero dependencies, ~13KB bundled

## Skills

Always read and follow `.claude/skills/tina4-js/SKILL.md` when working with this project.
"#;
        if fs::write(claude_path, content).is_ok() {
            println!("  {} Created CLAUDE.md", icon_ok());
        }
    }

    // Install tina4-js skill
    let skill_dir = Path::new(".claude/skills/tina4-js");
    if skill_dir.exists() && !force {
        println!("  {} tina4-js skill already installed", icon_ok());
    } else {
        // Try to copy from the tina4-js repo or create a basic one
        let skill_source = dirs_next::home_dir()
            .map(|h| h.join("IdeaProjects/tina4-js/.claude/skills/tina4-js"))
            .filter(|p| p.exists());

        if let Some(src) = skill_source {
            // Copy the whole skill directory
            fn copy_dir(src: &Path, dst: &Path) {
                let _ = fs::create_dir_all(dst);
                if let Ok(entries) = fs::read_dir(src) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let dest = dst.join(entry.file_name());
                        if path.is_dir() {
                            copy_dir(&path, &dest);
                        } else {
                            let _ = fs::copy(&path, &dest);
                        }
                    }
                }
            }
            copy_dir(&src, skill_dir);
            println!("  {} Installed tina4-js skill from local repo", icon_ok());
        } else {
            let _ = fs::create_dir_all(skill_dir);
            let skill_content = "# tina4-js Skill\n\nUse tina4-js signals, Tina4Element, html tagged templates, and the built-in router.\n\nSee https://tina4.com for documentation.\n";
            let _ = fs::write(skill_dir.join("SKILL.md"), skill_content);
            println!("  {} Created basic tina4-js skill", icon_ok());
        }
    }

    println!("  {} AI context installed for tina4-js project", icon_ok());
}
