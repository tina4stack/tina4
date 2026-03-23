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
    version = "3.2.0",
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

fn handle_update() {
    println!("{} Checking for updates...", icon_play().green());
    println!(
        "{} Self-update not yet configured. Download from: https://github.com/tina4stack/tina4/releases",
        icon_info().blue()
    );
}
