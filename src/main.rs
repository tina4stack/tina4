mod detect;
mod doctor;
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

    /// Scaffold a new Tina4 project
    Init {
        /// Project directory name
        dir: String,
        /// Language: python, php, ruby, nodejs
        #[arg(short, long)]
        lang: String,
    },

    /// Start the dev server with file watcher and SCSS compilation
    Serve {
        /// Port number (default: 7145)
        #[arg(short, long, default_value = "7145")]
        port: u16,
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

    /// Self-update the tina4 binary
    Update,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => doctor::run(),

        Commands::Install { lang } => install::run(&lang),

        Commands::Init { dir, lang } => handle_init(&dir, &lang),

        Commands::Serve { port } => handle_serve(port),

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

        Commands::Update => handle_update(),
    }
}

// ── Init ─────────────────────────────────────────────────────────

fn handle_init(dir: &str, lang: &str) {
    let lang_norm = lang.to_lowercase();
    let cli_name = match lang_norm.as_str() {
        "python" | "py" => "tina4python",
        "php" => "tina4php",
        "ruby" | "rb" => "tina4ruby",
        "nodejs" | "node" | "js" | "typescript" | "ts" => "tina4nodejs",
        _ => {
            eprintln!(
                "{} Unknown language: {}. Use: python, php, ruby, nodejs",
                "✗".red(),
                lang
            );
            std::process::exit(1);
        }
    };

    if which::which(cli_name).is_err() {
        eprintln!(
            "{} {} not found. Run: tina4 install {}",
            "✗".red(),
            cli_name,
            lang_norm
        );
        std::process::exit(1);
    }

    println!(
        "{} Scaffolding {} project in {}/",
        "▶".green(),
        lang_norm.cyan(),
        dir.cyan()
    );

    let status = std::process::Command::new(cli_name)
        .args(["init", dir])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("{} Project created at {}/", "✓".green(), dir);
            let scss_dir = format!("{}/src/scss", dir);
            let css_dir = format!("{}/src/public/css", dir);
            if std::path::Path::new(&scss_dir).exists() {
                scss::compile_dir(&scss_dir, &css_dir, false);
            }
        }
        Ok(s) => {
            eprintln!("{} Init failed (exit {:?})", "✗".red(), s.code());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{} Failed to run {}: {}", "✗".red(), cli_name, e);
            std::process::exit(1);
        }
    }
}

// ── Serve ────────────────────────────────────────────────────────

fn handle_serve(port: u16) {
    let lang = detect::detect_language();

    let info = match lang {
        Some(i) => i,
        None => {
            eprintln!(
                "{} No Tina4 project detected. Run: tina4 init <dir> --lang <language>",
                "✗".red()
            );
            std::process::exit(1);
        }
    };

    println!(
        "{} Detected {} project",
        "✓".green(),
        info.language.cyan()
    );

    // Compile SCSS
    let scss_dir = "src/scss";
    let css_dir = "src/public/css";
    if std::path::Path::new(scss_dir).exists() {
        scss::compile_dir(scss_dir, css_dir, false);
    }

    // Start language server
    let cli = info.cli_name();
    println!(
        "{} Starting {} on port {}",
        "▶".green(),
        cli.cyan(),
        port.to_string().yellow()
    );

    let mut server = match start_language_server(&info, port) {
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
    watcher::watch_and_reload(scss_dir, css_dir, &info, port, &mut server);
}

fn start_language_server(
    info: &detect::ProjectInfo,
    port: u16,
) -> Option<std::process::Child> {
    let port_s = port.to_string();

    let result = match info.language.as_str() {
        "python" => std::process::Command::new("python")
            .args(["app.py", &port_s])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn(),
        "php" => {
            let addr = format!("0.0.0.0:{}", port);
            std::process::Command::new("php")
                .args(["-S", &addr, "-t", "."])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
        }
        "ruby" => std::process::Command::new("tina4ruby")
            .args(["start", &port_s])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn(),
        "nodejs" => std::process::Command::new("npx")
            .args(["tsx", "app.ts"])
            .env("PORT", &port_s)
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
