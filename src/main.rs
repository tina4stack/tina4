pub mod console;
mod agent;
mod deploy;
mod env_config;
mod env_migrate;
mod detect;
mod doctor;
mod init;
mod install;
mod manifest;
mod mcp_context;
mod metrics;
mod rag;
mod scss;
mod session;
mod setup;
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

    /// Guided, menu-driven setup: install everything + scaffold a ready-to-run project
    Setup {
        /// Show the menu and plan without installing or scaffolding anything (for testing)
        #[arg(long)]
        dry_run: bool,
        /// Run the real menu + scaffold + CLAUDE.md, but skip all system installs (safe local test)
        #[arg(long)]
        skip_install: bool,
        /// Internal: this is the elevated (Administrator) re-run. Skips the menu
        /// and takes answers from the flags below. Set by the UAC relaunch —
        /// env vars do NOT survive Start-Process -Verb RunAs, so answers ride
        /// on argv instead. (hidden from --help)
        #[arg(long, hide = true)]
        elevated: bool,
        /// Internal: pre-selected language for the elevated re-run.
        #[arg(long, hide = true)]
        lang: Option<String>,
        /// Internal: pre-selected AI tool (desktop|code|none) for the elevated re-run.
        #[arg(long = "ai", hide = true)]
        ai: Option<String>,
        /// Internal: projects folder for the elevated re-run.
        #[arg(long = "projects-dir", hide = true)]
        projects_dir: Option<String>,
        /// Internal: project name for the elevated re-run.
        #[arg(long = "name", hide = true)]
        name: Option<String>,
    },

    /// Install a language runtime (python, php, ruby, nodejs)
    Install {
        /// Language to install: python, php, ruby, nodejs
        lang: String,
    },

    /// Scaffold a new Tina4 project: tina4 init <language> <path>
    Init {
        /// Language: python, php, ruby, nodejs, js (tina4-js frontend SPA)
        lang: Option<String>,
        /// Project directory (absolute or relative path)
        path: Option<String>,
    },

    /// Start the server with file watcher and SCSS compilation.
    /// Production servers are auto-detected; use --dev to force the dev server.
    Serve {
        /// Optional project name — resolved against the current folder, then
        /// your configured projects folder, and cd'd into before serving.
        #[arg(value_name = "PROJECT")]
        project: Option<String>,

        /// Port number (default: auto per framework — php:7145, python:7146, ruby:7147, nodejs:7148)
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

        /// Disable hot-reload signal from the file watcher (sets
        /// TINA4_NO_RELOAD=true). Useful in stable demos and CI.
        #[arg(long)]
        no_reload: bool,
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

    /// Start the AI agent server for Code With Me
    Agent {
        /// Port number (default: framework port + 2000)
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Build production assets (SCSS minify + bundle for the front-end)
    Build {
        /// Disable minification (default: minify in production builds)
        #[arg(long)]
        no_minify: bool,
    },

    /// Generate deployment scaffolding (Dockerfile, systemd unit,
    /// nginx server block, or cPanel .htaccess + README)
    Deploy {
        /// Target environment: docker, systemd, nginx, cpanel
        #[arg()]
        target: String,
        /// PHP runtime for `deploy docker`: cli (default), fpm, or swoole.
        /// Ignored for the other languages and targets.
        #[arg(long)]
        runtime: Option<String>,
        /// Overwrite existing files instead of skipping them
        #[arg(long)]
        force: bool,
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
        /// Migrate legacy un-prefixed env vars (DATABASE_URL, SECRET,
        /// SMTP_*, IMAP_*, HOST_NAME, SWAGGER_*, ORM_*) to their
        /// TINA4_* canonical names. Writes a .env.bak backup first.
        #[arg(long)]
        migrate: bool,
        /// Skip confirmation prompts (use with --migrate in CI scripts).
        #[arg(long)]
        yes: bool,
    },

    /// Report code-health top offenders — NATIVE, language-agnostic (ADR-0002).
    /// Scans SOURCE directly (Python/PHP/Ruby/TypeScript+JS) with NO Tina4 project
    /// and NO running framework required: per-file LOC, cyclomatic complexity,
    /// maintainability index, coupling, function count, offenders + `--fail-on`.
    Metrics {
        /// Directory OR file to scan (default: cwd, auto-detecting src/ or packages/*/src)
        #[arg(long)]
        path: Option<String>,
        /// Exit 1 when any offender is at/above this severity (CI gate)
        #[arg(long = "fail-on", value_name = "warn|error")]
        fail_on: Option<String>,
        /// Machine-readable JSON (matches the dev-admin dashboard shape)
        #[arg(long)]
        json: bool,
        /// Show only the worst N offenders (default: 20)
        #[arg(long)]
        top: Option<usize>,
    },

    /// Any command the client doesn't own is forwarded verbatim to the detected
    /// framework CLI: `tina4 <cmd> <args...>` -> `<framework-cli> <cmd> <args...>`.
    /// This covers migrate/migrate:create/seed/test/routes/queue/
    /// generate/console and anything a framework adds later. The framework owns
    /// arg parsing and rejects unknowns; dispatch pays zero manifest cost.
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() {
    console::enable_ansi();

    // Top-level help is the ONLY place the manifest is consumed: render clap's
    // help for the native conductor commands, then append the framework's
    // DISCOVERED commands. `--refresh` re-queries past the fingerprint cache.
    // Everything else goes straight to clap + dispatch (zero manifest cost).
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let first = raw_args.first().map(String::as_str);
    let wants_help = raw_args.is_empty()
        || matches!(first, Some("-h" | "--help" | "help"))
        || raw_args.iter().all(|a| a == "--refresh");
    if wants_help {
        let refresh = raw_args.iter().any(|a| a == "--refresh");
        print_help(refresh);
        return;
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => doctor::run(),

        Commands::Setup { dry_run, skip_install, elevated, lang, ai, projects_dir, name } => {
            setup::run(setup::SetupArgs {
                dry_run,
                skip_install,
                elevated,
                lang,
                ai,
                projects_dir,
                name,
            })
        }

        Commands::Install { lang } => install::run(&lang),

        Commands::Init { lang, path } => init::run(lang.as_deref(), path.as_deref()),

        Commands::Serve { project, port, host, dev, production, no_browser, no_reload } => {
            // `tina4 serve <projectname>` — resolve the named project and
            // change into it before serving. Look in the current folder
            // first (./<name>), then the configured projects folder.
            if let Some(name) = project {
                let cwd_candidate = std::env::current_dir()
                    .map(|d| d.join(&name))
                    .ok()
                    .filter(|p| p.is_dir());
                let resolved = cwd_candidate.or_else(|| {
                    crate::setup::configured_projects_dir()
                        .map(|d| d.join(&name))
                        .filter(|p| p.is_dir())
                });
                match resolved {
                    Some(path) => {
                        if let Err(e) = std::env::set_current_dir(&path) {
                            eprintln!(
                                "{} Could not enter {}: {}",
                                icon_fail().red(),
                                path.display(),
                                e
                            );
                            std::process::exit(1);
                        }
                        println!("{} Serving {}", icon_play().green(), path.display().to_string().cyan());
                    }
                    None => {
                        let dir = crate::setup::configured_projects_dir()
                            .map(|d| d.display().to_string())
                            .unwrap_or_else(|| "(none configured)".to_string());
                        eprintln!(
                            "{} Project '{}' not found in the current folder or your projects folder ({})",
                            icon_fail().red(),
                            name,
                            dir
                        );
                        std::process::exit(1);
                    }
                }
            } else if detect::detect_language().is_none() {
                // Bare `tina4 serve` and the current folder is NOT a Tina4
                // project. Fall back to the configured projects folder: if it
                // holds exactly one project, cd into it and serve; if several,
                // list them and ask the user to name one; if none, guide them.
                if let Some(projects_dir) = crate::setup::configured_projects_dir() {
                    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(&projects_dir)
                        .map(|rd| {
                            rd.filter_map(|e| e.ok().map(|e| e.path()))
                                .filter(|p| p.is_dir() && detect::dir_is_tina4_project(p))
                                .collect()
                        })
                        .unwrap_or_default();
                    found.sort();
                    match found.len() {
                        1 => {
                            let path = &found[0];
                            if let Err(e) = std::env::set_current_dir(path) {
                                eprintln!("{} Could not enter {}: {}", icon_fail().red(), path.display(), e);
                                std::process::exit(1);
                            }
                            println!("{} Serving {}", icon_play().green(), path.display().to_string().cyan());
                        }
                        0 => {
                            eprintln!(
                                "{} Not in a Tina4 project, and no projects found in {}.",
                                icon_fail().red(),
                                projects_dir.display()
                            );
                            eprintln!("  Run {} to scaffold one, or cd into a project first.", "tina4 setup".cyan());
                            std::process::exit(1);
                        }
                        _ => {
                            eprintln!(
                                "{} Not in a Tina4 project. Projects in {}:",
                                icon_info().blue(),
                                projects_dir.display()
                            );
                            for p in &found {
                                if let Some(n) = p.file_name().and_then(|s| s.to_str()) {
                                    eprintln!("    {}", n.cyan());
                                }
                            }
                            eprintln!("  Run {} to serve one.", "tina4 serve <name>".cyan());
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!(
                        "{} Not in a Tina4 project (no app.py / index.php / app.rb / app.ts here).",
                        icon_fail().red()
                    );
                    eprintln!("  cd into a project, or run {} first.", "tina4 setup".cyan());
                    std::process::exit(1);
                }
            }
            // --no-reload is the flag form of TINA4_NO_RELOAD=true.
            // Set it in the environment before handing off to the
            // server bootstrap so the watcher and language CLI both
            // see it.
            if no_reload {
                std::env::set_var("TINA4_NO_RELOAD", "true");
            }
            // TINA4_NO_BROWSER is now set for the child unconditionally inside
            // handle_serve — the CLI opens the browser, the framework must not.
            // The conditional set that used to live here only covered the
            // --no-browser case, which left a plain `serve` opening a duplicate
            // tab from the framework side.
            handle_serve(port, &host, dev, production, no_browser);
        }

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

        // Native, language-agnostic metrics engine (ADR-0002). No longer
        // forwarded to the framework CLI — scans SOURCE directly, no project
        // or running framework required.
        Commands::Metrics { path, fail_on, json, top } => {
            std::process::exit(metrics::run(path, top, json, fail_on));
        }

        // Any non-native command (migrate, migrate:create, seed, test, routes,
        // queue, generate, console, ...) is forwarded verbatim to the
        // framework CLI. The framework owns arg parsing and rejects unknowns, so
        // the client carries no per-command flag knowledge and can never drift
        // out of parity with the framework's command surface.
        Commands::External(args) => delegate_command(args),

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

        Commands::Books => handle_books(),
        Commands::Docs => handle_docs(),
        Commands::Build { no_minify } => {
            // For now `build` runs the SCSS compiler in minify-by-default
            // mode and delegates to the language CLI for any
            // language-specific bundling step (Node bundles via
            // `tina4nodejs build`, etc.). This stays small intentionally —
            // the alternative is dragging a full asset pipeline in.
            scss::compile_dir("src/scss", "src/public/css", !no_minify);
            // Try the language CLI's build subcommand; if the language
            // doesn't have one, the delegate exits non-zero and we
            // surface that to the user — they get to decide whether
            // to wire one up.
            delegate_command(vec!["build".into()]);
        }

        Commands::Deploy { target, runtime, force } => deploy::run(&target, runtime.as_deref(), force),

        Commands::Env { sync, example, list, migrate, yes } => {
            if migrate {
                env_migrate::run(yes);
            } else {
                env_config::run(sync, example, list);
            }
        }
    }
}

// ── Help (native + discovered) ───────────────────────────────────

/// Render `tina4 --help`: clap's help for the native conductor commands, then
/// the commands DISCOVERED from the detected framework's `commands --json`
/// manifest. The manifest is consumed HERE ONLY — dispatch never touches it, so
/// a discovery miss just yields a shorter listing, never a broken command.
/// `refresh` re-queries past the fingerprint cache.
fn print_help(refresh: bool) {
    use clap::CommandFactory;
    let mut command = Cli::command();
    print!("{}", command.render_long_help());

    let info = match detect::detect_language() {
        Some(info) => info,
        None => return,
    };
    let discovered = match manifest::load(&info, refresh) {
        Some(manifest) => manifest,
        None => return,
    };

    // Skip commands clap already prints (the native conductor set) plus the
    // framework's own `help` — list only what's genuinely extra.
    let native: std::collections::HashSet<String> = command
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let extra: Vec<&manifest::Command> = discovered
        .commands
        .iter()
        .filter(|c| c.name != "help" && !native.contains(&c.name))
        .collect();
    if extra.is_empty() {
        return;
    }

    println!(
        "\nDiscovered from {} {} (via {}):",
        discovered.framework.cyan(),
        discovered.version.dimmed(),
        info.cli_name()
    );
    let width = extra.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for command in extra {
        println!("  {:<width$}  {}", command.name, command.summary);
        if !command.subcommands.is_empty() {
            println!("  {:<width$}    {}", "", command.subcommands.join(", ").dimmed());
        }
    }
}

// ── Serve ────────────────────────────────────────────────────────

/// The environment `--production` puts in front of the framework.
///
/// Returned rather than set, so it can be asserted without mutating the
/// process: `handle_serve` takes over the process once it spawns the server,
/// which leaves no seam to test through otherwise.
fn production_env() -> [(&'static str, &'static str); 2] {
    [
        // No dev toolbar, no error overlay, no template recompilation.
        ("TINA4_DEBUG", "false"),
        // The framework's signal that the CLI meant production, as opposed to
        // inferring it from TINA4_DEBUG being false (which a plain
        // `tina4 serve` also produces on a project that sets it). Node forks a
        // worker per CPU core on this.
        ("TINA4_PRODUCTION", "true"),
    ]
}

pub fn handle_serve(port: Option<u16>, host: &str, force_dev: bool, force_production: bool, no_browser: bool) {
    // The CLI owns browser-opening. Decide OUR answer here, before any child is
    // spawned, then suppress the child's unconditionally.
    //
    // Every framework also opens a browser on listen unless TINA4_NO_BROWSER is
    // set, and we only propagated that when --no-browser was passed. So a plain
    // `tina4 serve` opened THREE tabs: ours for the app, ours for /__dev, and
    // the framework's duplicate of the app. Setting the var on the child after
    // reading our own answer collapses that to the two we intend.
    //
    // The read must happen BEFORE the set_var below, or we would suppress
    // ourselves along with the child.
    let env_no_browser = read_dotenv_bool("TINA4_NO_BROWSER");
    let os_no_browser = std::env::var("TINA4_NO_BROWSER")
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(false);
    let cli_opens_browser = !(no_browser || env_no_browser || os_no_browser);
    // Inherited by every spawn site in start_language_server, so a new runtime
    // cannot forget it the way a per-Command .env() call could.
    std::env::set_var("TINA4_NO_BROWSER", "true");

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
        for (key, value) in production_env() {
            std::env::set_var(key, value);
        }
        // TINA4_PRODUCTION is how a framework knows the CLI meant production,
        // as opposed to merely inferring it from TINA4_DEBUG being false (which
        // is also true for a plain `tina4 serve` on a project that sets it).
        //
        // Node reads it to decide whether to fork a worker per CPU core. Nothing
        // set it, so `tina4 serve --production` ran ONE process on ONE core, and
        // the cluster path was unreachable from the CLI - which is also why its
        // never-worked bug went unnoticed for so long. Measured before this
        // line existed: `--production` printed a plain banner; only
        // `TINA4_PRODUCTION=true tina4 serve --production` printed
        // "(cluster, 8 workers)".
        //
        // Python, PHP and Ruby do not read it, so this is a no-op for them
        // today. It is set unconditionally rather than per-language so the
        // meaning stays "the operator asked for production", not "Node needs a
        // flag" - the next framework to want the distinction gets it free.
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

    // Start the AI agent server in background (for Code With Me).
    // Agent port = framework port + 2000.
    //
    // Gated on BOTH:
    //   1. `--production` CLI flag is NOT set (explicit user intent)
    //   2. TINA4_DEBUG is truthy
    //
    // Either gate alone would suffice in normal usage — `--production`
    // sets TINA4_DEBUG=false a few lines up — but we check both so
    // that a stale shell env or weird wrapper script can't leak the
    // agent port into a production deployment. The agent exposes LLM
    // endpoints, plan execution, and file-write tools; shipping it
    // in prod would be a serious footgun.
    let debug_mode = std::env::var("TINA4_DEBUG")
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false);
    let allow_agents = debug_mode && !force_production;
    if allow_agents {
        let agent_port = port + 2000;
        std::thread::spawn(move || {
            match std::panic::catch_unwind(|| {
                agent::run(agent_port);
            }) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("  {} Agent server crashed: {:?}", console::icon_warn(), e);
                }
            }
        });
    } else {
        let reason = if force_production {
            "--production flag set"
        } else {
            "TINA4_DEBUG=false"
        };
        println!(
            "  {} Agent server disabled ({})",
            icon_info().blue(),
            reason
        );
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

    // Spawn the first language server.
    let initial_child = match start_language_server(&info, port, host) {
        Some(c) => c,
        None => {
            // The spawn error itself is printed by start_language_server. This
            // message alone used to be the ENTIRE output on failure, which is
            // how a missing `uv` in a container looked identical to a missing
            // interpreter, a bad entry point, or a permissions problem. Never
            // report a failure without the reason.
            eprintln!(
                "{} Failed to start the {} server. See the error above.",
                icon_fail().red(),
                info.language.cyan()
            );
            std::process::exit(1);
        }
    };

    // ── Child supervision ──────────────────────────────────────────
    // The CLI owns the language dev server as a child process. File
    // changes do NOT respawn it: the watcher POSTs /__dev/api/reload and
    // the framework re-imports changed modules in-process (see
    // watcher.rs), so the server stays up and dashboard/WebSocket
    // connections survive. This loop's job is supervision -- detect when
    // the child exits on its own (a crash) and surface the error.
    //
    // The respawn_requested flag below is retained as dormant
    // infrastructure: the watcher no longer sets it, so the respawn
    // branch never fires today. It is kept (cheaply) for a future
    // crash-recovery path, and the group-kill it performs matches the
    // shutdown handler so it can never orphan the child tree if revived.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut current_child = initial_child;
    let respawn_requested = Arc::new(AtomicBool::new(false));

    // Track the current child's pid (== its process-group id, set via setpgid)
    // so the shutdown handler can tear down the WHOLE dev-server tree (npx -> tsx
    // -> node, uv -> python, bundle -> ruby). The child sits in its own process
    // group, so a signal sent to this CLI never propagates to it — without an
    // explicit group kill the tree is orphaned every time `serve` is stopped.
    // The ctrlc "termination" feature makes this fire for SIGINT (Ctrl-C),
    // SIGTERM (`kill`, supervisor stop) and SIGHUP (terminal closed) — the three
    // ways a dev server actually dies. SIGKILL stays uncatchable; nothing can
    // reap on -9, so the OS reparents the group to init in that one case.
    let current_pid = Arc::new(std::sync::atomic::AtomicI32::new(current_child.id() as i32));
    {
        let cp = Arc::clone(&current_pid);
        let _ = ctrlc::set_handler(move || {
            let pid = cp.load(Ordering::SeqCst);
            if pid > 0 {
                terminate_group(pid as u32);
            }
            std::process::exit(130);
        });
    }

    // Give the server a moment to bind, then open browser.
    //
    // Three ways to suppress, all folded into `cli_opens_browser` at the top of
    // this function (it must be computed before we set TINA4_NO_BROWSER for the
    // child, or we would read our own suppression back):
    //   1. `--no-browser` CLI flag
    //   2. `TINA4_NO_BROWSER=true` in the process environment
    //   3. `TINA4_NO_BROWSER=true` in the project's .env file
    //
    // The .env read mirrors what the framework side does, so a single
    // entry in .env governs both browser-open points (fixes tina4-book#131).
    std::thread::sleep(std::time::Duration::from_secs(2));
    let url = format!("http://localhost:{}", port);
    if !cli_opens_browser {
        println!("{} Server ready: {}", icon_ok().green(), url.cyan());
    } else {
        console::open_browser(&url);
        println!("{} Browser opened: {}", icon_ok().green(), url.cyan());
        // Also open the dev dashboard in a second tab — the route inspector, DB
        // runner, logs, AI chat and live tools, right next to the running app.
        // Dev only: `/__dev` doesn't exist in a production server.
        if !force_production {
            let dashboard = format!("{}/__dev", url);
            std::thread::sleep(std::time::Duration::from_millis(400));
            console::open_browser(&dashboard);
            println!("{} Dashboard:      {}", icon_ok().green(), dashboard.cyan());
        }
    }

    // File watcher — the Rust CLI owns all file watching. On change it:
    //   1. Compiles SCSS (if src/scss/ exists)
    //   2. POSTs /__dev/api/reload so the framework hot-reloads in-process
    //      (code re-imports, assets refresh) -- the server is NOT restarted
    //   3. The framework then pushes a reload to the browser via its
    //      WebSocket / mtime endpoint
    // Production runs NO watchers. File/SCSS watching burns CPU, contends with
    // the single server process (measurably throttling throughput), and there is
    // nothing to hot-reload in production. SCSS was already compiled once above.
    // The CLI stays only to supervise the child server + clean-shutdown its tree.
    if force_production {
        println!(
            "{} Production mode — file watching + hot-reload disabled; press Ctrl+C to stop",
            icon_play().green()
        );
    } else {
        match info.language.as_str() {
            "tina4js" => println!(
                "{} Vite HMR active — press Ctrl+C to stop",
                icon_eye().green()
            ),
            _ => println!(
                "{} File watcher active (changes hot-reload in-process) — press Ctrl+C to stop",
                icon_eye().green()
            ),
        }

        // SCSS compile-on-save in a background thread (dev only)
        if std::path::Path::new(scss_dir).exists() {
            let scss_in = scss_dir.to_string();
            let css_out = css_dir.to_string();
            std::thread::spawn(move || {
                watcher::watch_scss(&scss_in, &css_out, false);
            });
        }

        // File change watcher — gets a shared atomic flag it can set on
        // code-file changes. Main polls the flag below. (dev only)
        if info.language != "tina4js" {
            let reload_port = port;
            let watcher_flag = Arc::clone(&respawn_requested);
            std::thread::spawn(move || {
                watcher::watch_and_reload(reload_port, Some(watcher_flag));
            });
        }
    }

    // ── Respawn loop ───────────────────────────────────────────────
    // Polls the current child every 250ms with try_wait() AND checks
    // the respawn_requested flag. If the flag is set, kill the child
    // we own and spin up a fresh one. If the child exits naturally
    // (non-zero), surface the error and bail.
    let info_clone = info.clone();
    let host_clone = host.to_string();
    loop {
        // Inner poll loop — exits when either:
        //   (a) child exited naturally (status returned by try_wait)
        //   (b) watcher set respawn_requested → we kill + break
        let exit_cause: ExitCause = loop {
            // Dormant respawn branch (the watcher no longer sets this flag --
            // see the supervision note above). If ever revived, it kills the
            // child's whole process GROUP before respawning: Child::kill would
            // signal only the direct PID (npx), orphaning the tsx -> node
            // grandchildren. terminate_group reaps the entire tree.
            if respawn_requested.swap(false, Ordering::SeqCst) {
                terminate_group(current_child.id());
                let _ = current_child.wait(); // reap the direct child
                current_pid.store(0, Ordering::SeqCst);
                break ExitCause::Respawn;
            }
            // Child exited on its own?
            match current_child.try_wait() {
                Ok(Some(status)) => break ExitCause::Exited(status),
                Ok(None) => { /* still running */ }
                Err(e) => break ExitCause::WaitError(e),
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };

        match exit_cause {
            ExitCause::Respawn => {
                println!("{} Respawning {} after code change…",
                    icon_play().green(), info_clone.cli_name().cyan());
                std::thread::sleep(std::time::Duration::from_millis(200));
                match start_language_server(&info_clone, port, &host_clone) {
                    Some(new_child) => {
                        current_child = new_child;
                        current_pid.store(current_child.id() as i32, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        let body = r#"{"type":"reload","file":"<respawn>"}"#;
                        let url = format!("http://127.0.0.1:{}/__dev/api/reload", port);
                        let _ = watcher::ureq_post(&url, body);
                        println!("{} Server ready: {}", icon_ok().green(),
                            format!("http://localhost:{}", port).cyan());
                        // Loop back — keep polling the new child
                    }
                    None => {
                        eprintln!("{} Respawn failed — child wouldn't start. Fix the error and re-run.",
                            icon_fail().red());
                        std::process::exit(1);
                    }
                }
            }
            ExitCause::Exited(status) if status.success() => break,
            ExitCause::Exited(status) => {
                let code = status.code().unwrap_or(-1);
                eprintln!(
                    "\n{} Server process exited with code {}. Check logs/error.log or your PHP/Python error log for details.",
                    icon_fail().red(),
                    code
                );
                std::process::exit(code);
            }
            ExitCause::WaitError(e) => {
                eprintln!("\n{} Server process error: {}", icon_fail().red(), e);
                std::process::exit(1);
            }
        }
    }
}

enum ExitCause {
    Respawn,
    Exited(std::process::ExitStatus),
    WaitError(std::io::Error),
}

/// Read a boolean-valued variable from the given `.env` file.
/// Returns true for values `true` / `1` / `yes` (case-insensitive), false otherwise.
/// Pure function of the path and contents — easy to test.
fn read_dotenv_bool_from<P: AsRef<std::path::Path>>(path: P, key: &str) -> bool {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches('"').trim_matches('\'').to_lowercase();
                return matches!(v.as_str(), "true" | "1" | "yes");
            }
        }
    }
    false
}

/// Convenience: read from `.env` in the current working directory.
/// Used by `handle_serve` so env vars like `TINA4_NO_BROWSER` can live in
/// `.env` and govern both the CLI's browser-open and the framework's.
fn read_dotenv_bool(key: &str) -> bool {
    read_dotenv_bool_from(".env", key)
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

/// Terminate the dev server's entire process GROUP, not just the direct child.
///
/// The child is spawned as its own group leader (`set_process_group` →
/// `setpgid(0,0)`), so its process-group id equals its pid. `Child::kill()`
/// signals only that one pid — which for Node is the `npx` wrapper, leaving the
/// `tsx` and `node app.ts` grandchildren orphaned (reparented to init). That is
/// the leak that accumulated ~one dead `node app.ts` per serve shutdown
/// (Ctrl-C, `kill`, or a closed terminal). `killpg` reaps the whole
/// `npx -> tsx -> node` tree: SIGTERM for a graceful stop, then SIGKILL for
/// anything still alive after a short grace period.
#[cfg(unix)]
fn terminate_group(pid: u32) {
    let pgid = pid as i32;
    unsafe {
        libc::killpg(pgid, libc::SIGTERM);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
}

/// Windows has no process groups in the POSIX sense — kill the tree via taskkill.
#[cfg(not(unix))]
fn terminate_group(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

fn start_language_server(
    info: &detect::ProjectInfo,
    port: u16,
    host: &str,
) -> Option<std::process::Child> {
    let port_s = port.to_string();

    let result = match info.language.as_str() {
        "python" => {
            // A .venv means "use THAT interpreter" -- it does NOT mean uv is
            // installed. The old code jumped straight to `uv run` whenever
            // .venv existed, so any environment holding a venv but no uv could
            // not start at all: spawn failed, this returned None, and the
            // caller printed a bare "Failed to start server" with no cause.
            // A production container is exactly that shape -- uv builds the
            // venv in the builder stage and is deliberately left out of the
            // slim runtime -- so `tina4 serve` could never boot a Python image.
            //
            // Run the venv's own interpreter directly. It needs no uv, and it
            // is the same interpreter `uv run` would have selected anyway.
            let venv_python = if cfg!(windows) {
                std::path::PathBuf::from(".venv").join("Scripts").join("python.exe")
            } else {
                std::path::PathBuf::from(".venv").join("bin").join("python")
            };
            if venv_python.exists() {
                let mut cmd = std::process::Command::new(&venv_python);
                cmd.args(["app.py", "--managed"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            } else if std::path::Path::new(".venv").exists() && which::which("uv").is_ok() {
                // A .venv with no interpreter inside it (a partial or foreign
                // layout). uv can still resolve it -- but only if uv is here.
                let mut cmd = std::process::Command::new("uv");
                cmd.args(["run", "python", "app.py", "--managed"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            } else {
                let mut cmd = std::process::Command::new(console::python_cmd());
                cmd.args(["app.py", "--managed"])
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
            let (cmd_name, mut cmd_args) = resolve_cli(info);
            cmd_args.extend([
                "serve".into(),
                "--managed".into(),
                "--host".into(), host.into(),
                "--port".into(), port.to_string(),
            ]);
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
                // HELD BACK, deliberately. `ruby app.rb` brings Puma up
                // WITHOUT the framework's built-in routes, so a container
                // launched this way answers 404 on /health while the same image
                // serves 200 under `tina4ruby serve` -- PHP already goes through
                // its framework CLI for exactly that reason. Switching Ruby over
                // is the right shape AND the gem now accepts --managed, but the
                // supervised child still exits shortly after Puma binds, and
                // this code path is what every Ruby dev hits on `tina4 serve`,
                // not just containers. Shipping it would trade a container-only
                // fault for a broken local dev loop. Restore the tina4ruby form
                // once the parent/child lifecycle is understood.
                let mut cmd = std::process::Command::new(console::resolve_cmd("bundle"));
                cmd.args(["exec", "ruby", "app.rb", "--managed"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                set_process_group(&mut cmd).spawn()
            } else {
                let mut cmd = std::process::Command::new("ruby");
                cmd.args(["app.rb", "--managed"])
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
            // COMPILED OUTPUT FIRST. `npx tsx` is a development convenience: it
            // transpiles TypeScript at run time and costs a whole process tree
            // to do it. Measured in a production container it was five
            // processes (npm exec -> tsx -> node -> esbuild service) at ~455 MB
            // RSS, a 256m memory floor where the other three frameworks live on
            // 64m, and the slowest throughput of the four. And because
            // `npm ci --omit=dev` strips tsx, npx FETCHED it over the network at
            // container start -- a boot that fails outright when air-gapped.
            //
            // So: run dist/app.js when it exists, which is what a built image
            // ships. tsx stays as the DEV path, where transpile-on-save is the
            // point and the dev dependency is installed.
            let built = std::path::Path::new("dist/app.js");
            if built.exists() {
                let mut cmd = std::process::Command::new(console::resolve_cmd("node"));
                cmd.args(["dist/app.js", "--managed"])
                    .env("PORT", &port_s)
                    .env("HOST", host)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                return set_process_group(&mut cmd).spawn().map_err(|e| {
                    eprintln!("  {} Could not spawn node on dist/app.js: {}", icon_fail().red(), e);
                    e
                }).ok();
            }
            let entry = if std::path::Path::new("app.ts").exists() { "app.ts" } else { "app.js" };
            let mut cmd = std::process::Command::new(console::resolve_cmd("npx"));
            cmd.args(["tsx", entry, "--managed"])
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

    // Print WHY the spawn failed instead of throwing the error away. `.ok()`
    // discarded it, so a missing interpreter, a missing `uv`, a bad path and a
    // permissions error all surfaced as the same blank "Failed to start
    // server" -- unactionable, and it hid a real container bug.
    match result {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!(
                "  {} Could not spawn the {} server: {}",
                icon_fail().red(),
                info.language.cyan(),
                e
            );
            None
        }
    }
}

// ── Delegate ─────────────────────────────────────────────────────

/// Resolve the language CLI command and arguments for the detected project.
/// PHP needs special handling: `php vendor/bin/tina4php` instead of bare `tina4php`.
pub(crate) fn resolve_cli(info: &detect::ProjectInfo) -> (String, Vec<String>) {
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
            if (std::path::Path::new("uv.lock").exists() || std::path::Path::new("pyproject.toml").exists())
                && which::which("uv").is_ok() {
                    return ("uv".into(), vec!["run".into(), "tina4python".into()]);
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
            if std::path::Path::new("Gemfile.lock").exists()
                && which::which("bundle").is_ok() {
                    return ("bundle".into(), vec!["exec".into(), "tina4ruby".into()]);
                }
            ("tina4ruby".into(), vec![])
        }
        "nodejs" => {
            // npm projects: run via 'npx tina4nodejs'
            if std::path::Path::new("node_modules").exists()
                && which::which("npx").is_ok() {
                    return ("npx".into(), vec!["tina4nodejs".into()]);
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
        "php" => std::process::Command::new(console::resolve_cmd("composer"))
            .args(["update", "tina4stack/tina4php"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        "ruby" => std::process::Command::new(console::resolve_cmd("bundle"))
            .args(["update", "tina4ruby"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        "nodejs" => std::process::Command::new(console::resolve_cmd("npm"))
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
        let skill_source = std::env::var("HOME").ok()
            .map(|h| Path::new(&h).join("IdeaProjects/tina4-js/.claude/skills/tina4-js"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp .env file and hand its path to the test. Each test
    /// uses a unique filename so parallel test runners don't collide.
    fn with_env_file<F: FnOnce(&std::path::Path)>(name: &str, contents: &str, f: F) {
        let path = std::env::temp_dir().join(format!("tina4-dotenv-{}-{}-{}", std::process::id(), name, fastrand_like()));
        std::fs::write(&path, contents).unwrap();
        f(&path);
        let _ = std::fs::remove_file(&path);
    }

    fn fastrand_like() -> u64 {
        // Tiny per-thread counter; avoids pulling in a rng crate.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn dotenv_bool_reads_true() {
        with_env_file("true", "TINA4_NO_BROWSER=true\n", |p| {
            assert!(read_dotenv_bool_from(p, "TINA4_NO_BROWSER"));
        });
    }

    #[test]
    fn dotenv_bool_reads_quoted_true() {
        with_env_file("qtrue", "TINA4_NO_BROWSER=\"true\"\n", |p| {
            assert!(read_dotenv_bool_from(p, "TINA4_NO_BROWSER"));
        });
    }

    #[test]
    fn dotenv_bool_reads_yes_and_one() {
        with_env_file("yesone", "A=yes\nB=1\n", |p| {
            assert!(read_dotenv_bool_from(p, "A"));
            assert!(read_dotenv_bool_from(p, "B"));
        });
    }

    #[test]
    fn dotenv_bool_returns_false_for_false() {
        with_env_file("false", "TINA4_NO_BROWSER=false\n", |p| {
            assert!(!read_dotenv_bool_from(p, "TINA4_NO_BROWSER"));
        });
    }

    #[test]
    fn dotenv_bool_returns_false_for_missing_key() {
        with_env_file("missing", "SOMETHING_ELSE=true\n", |p| {
            assert!(!read_dotenv_bool_from(p, "TINA4_NO_BROWSER"));
        });
    }

    #[test]
    fn dotenv_bool_returns_false_for_no_env_file() {
        assert!(!read_dotenv_bool_from("/does/not/exist/.env", "TINA4_NO_BROWSER"));
    }

    #[test]
    fn production_turns_debug_off() {
        let env = production_env();
        assert!(
            env.contains(&("TINA4_DEBUG", "false")),
            "--production must turn debug off; a production server with the dev \
             toolbar and error overlay on publishes its own stack traces"
        );
    }

    #[test]
    fn production_sets_tina4_production() {
        // Node reads TINA4_PRODUCTION to decide whether to fork a worker per
        // CPU core. Nothing set it, so `tina4 serve --production` ran ONE
        // process on ONE core and the cluster path was unreachable from the
        // CLI at all. Measured: without this the banner read
        // "Server: http://localhost:7148"; with it, "(cluster, 8 workers)".
        let env = production_env();
        assert!(
            env.contains(&("TINA4_PRODUCTION", "true")),
            "--production must set TINA4_PRODUCTION, or Node never clusters"
        );
    }

    #[test]
    fn production_env_sets_nothing_else() {
        // A guard on scope. Every variable here lands in every framework's
        // process, so an addition is a cross-language decision, not a detail.
        assert_eq!(production_env().len(), 2);
    }

    #[test]
    fn dotenv_bool_ignores_comments_and_blanks() {
        with_env_file("comments", "# comment\n\nTINA4_NO_BROWSER=yes\n# TINA4_DEBUG=true\n", |p| {
            assert!(read_dotenv_bool_from(p, "TINA4_NO_BROWSER"));
            assert!(!read_dotenv_bool_from(p, "TINA4_DEBUG"));
        });
    }
}
