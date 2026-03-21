mod detect;
mod doctor;
mod generate;
mod init;
mod install;
mod scss;
mod watcher;

use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(
    name = "tina4",
    version = "3.0.0",
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

    /// Self-update the tina4 binary
    Update,
}

fn main() {
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
                    "▶".green(),
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
                "✗".red()
            );
            std::process::exit(1);
        }
    };

    // Use framework-specific default port if not overridden
    let port = port.unwrap_or_else(|| info.default_port());

    println!(
        "{} Detected {} project",
        "✓".green(),
        info.language.cyan()
    );

    // Set TINA4_DEBUG=true when --dev flag is used, so the framework
    // CLI forces the dev server even if a production server is installed
    if force_dev {
        std::env::set_var("TINA4_DEBUG", "true");
        println!(
            "{} Dev mode forced — production server detection disabled",
            "ℹ".blue()
        );
    }

    // --production: install best production server if not available, force debug off
    if force_production {
        std::env::set_var("TINA4_DEBUG", "false");
        println!(
            "{} Production mode — installing best server if needed",
            "▶".green()
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
        "▶".green(),
        cli.cyan(),
        host.yellow(),
        port.to_string().yellow()
    );

    let mut server = match start_language_server(&info, port, host) {
        Some(child) => child,
        None => {
            eprintln!("{} Failed to start server", "✗".red());
            std::process::exit(1);
        }
    };

    // File watcher (blocks)
    println!(
        "{} File watcher active — src/, migrations/, .env",
        "👁".green()
    );
    watcher::watch_and_reload(scss_dir, css_dir, &info, port, host, &mut server);
}

fn install_production_server(info: &detect::ProjectInfo) {
    let (check_cmd, install_cmd, name) = match info.language.as_str() {
        "python" => ("uvicorn --version", "uv add uvicorn", "uvicorn"),
        "php" => ("php -m", "echo 'OPcache is built-in'", "opcache"),
        "ruby" => ("gem list puma", "gem install puma --no-doc", "puma"),
        "nodejs" => ("echo 'cluster is built-in'", "echo 'ok'", "cluster"),
        _ => return,
    };

    // Check if already installed
    let check = std::process::Command::new("sh")
        .args(["-c", check_cmd])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let needs_install = match info.language.as_str() {
        "python" => check.map(|s| !s.success()).unwrap_or(true),
        "ruby" => {
            let out = std::process::Command::new("sh")
                .args(["-c", "gem list puma | grep puma"])
                .output();
            out.map(|o| o.stdout.is_empty()).unwrap_or(true)
        }
        _ => false, // PHP opcache and Node cluster are built-in
    };

    if needs_install {
        println!(
            "  {} Installing {}...",
            "▶".green(),
            name.cyan()
        );
        let result = std::process::Command::new("sh")
            .args(["-c", install_cmd])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        match result {
            Ok(s) if s.success() => println!("  {} {} installed", "✓".green(), name.cyan()),
            _ => println!("  {} Failed to install {} — using dev server", "⚠".yellow(), name),
        }
    } else {
        println!("  {} {} already installed", "✓".green(), name.cyan());
    }
}

fn start_language_server(
    info: &detect::ProjectInfo,
    port: u16,
    host: &str,
) -> Option<std::process::Child> {
    let port_s = port.to_string();

    let result = match info.language.as_str() {
        "python" => std::process::Command::new("tina4python")
            .args(["serve", "--port", &port_s, "--host", host])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn(),
        "php" => {
            let addr = format!("{}:{}", host, port);
            std::process::Command::new("tina4php")
                .args(["serve", &addr])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
        }
        "ruby" => std::process::Command::new("tina4ruby")
            .args(["start", "--port", &port_s, "--host", host])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn(),
        "nodejs" => std::process::Command::new("npx")
            .args(["tsx", "app.ts"])
            .env("PORT", &port_s)
            .env("HOST", host)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn(),
        _ => return None,
    };

    result.ok()
}

// ── Delegate ─────────────────────────────────────────────────────

fn delegate_command(args: Vec<String>) {
    match detect::detect_language() {
        Some(info) => {
            let cli = info.cli_name();
            match std::process::Command::new(cli).args(&args).status() {
                Ok(s) if !s.success() => std::process::exit(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("{} Failed to run {}: {}", "✗".red(), cli, e);
                    std::process::exit(1);
                }
                _ => {}
            }
        }
        None => {
            eprintln!(
                "{} No Tina4 project detected in current directory",
                "✗".red()
            );
            std::process::exit(1);
        }
    }
}

// ── Update ───────────────────────────────────────────────────────

fn handle_update() {
    println!("{} Checking for updates...", "▶".green());
    println!(
        "{} Self-update not yet configured. Download from: https://github.com/tina4stack/tina4/releases",
        "ℹ".blue()
    );
}
