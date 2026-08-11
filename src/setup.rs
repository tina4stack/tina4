use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::console::{self, icon_info, icon_ok, icon_play, icon_warn};
use crate::{init, install};
use colored::Colorize;

/// The first prompt we seed a Claude Code session with (and show in CLAUDE.md)
/// so a brand-new project has an obvious, end-to-end thing to build.
const FIRST_PROMPT: &str = "Add a `/products` page backed by a `Product` model (name, price, image_url), seed three rows, and render them as cards using a tina4-js component.";

/// Which AI tool the developer wants to build with.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AiChoice {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    Cursor,
    All,
    None,
}

/// What the first run remembered, so later runs can skip straight to
/// "what type of project, what name".
struct SetupConfig {
    projects_dir: PathBuf,
    ai: AiChoice,
}

/// Guided, menu-driven setup — the "getting started" one-liner (install.ps1 /
/// install.sh) ends by calling this. A handful of questions, then it installs
/// the runtime (+ git, + the chosen AI tool, + the tina4 skills), creates a
/// projects folder, scaffolds a ready-to-run project inside it with its own
/// CLAUDE.md, and opens the tool so the user can start building.
///
/// Installs go through the OS package manager (Chocolatey on Windows, Homebrew
/// on macOS) so the user never types a package command.
///
/// Test safely before touching a real machine:
///   tina4 setup --dry-run       prints every step, changes nothing
///   tina4 setup --skip-install  real menu + scaffold + CLAUDE.md, no system installs
/// Answers + flags for `tina4 setup`. The `elevated`/`lang`/`ai`/`projects_dir`/
/// `name` fields carry the menu answers across the UAC boundary on Windows —
/// they ride on argv (set by the elevated relaunch) because environment
/// variables do NOT survive `Start-Process -Verb RunAs`.
pub struct SetupArgs {
    pub dry_run: bool,
    pub skip_install: bool,
    pub elevated: bool,
    pub lang: Option<String>,
    pub ai: Option<String>,
    pub projects_dir: Option<String>,
    pub name: Option<String>,
}

pub fn run(args: SetupArgs) {
    let dry_run = args.dry_run;
    let skip_install = args.skip_install;
    banner();

    // The elevated (Administrator) re-run hands its answers on argv — env vars
    // do NOT survive `Start-Process -Verb RunAs`. Detect it FIRST, before the
    // TTY guard and before load_config: an elevated re-run must skip every menu
    // regardless of whether a saved config exists, or the user would be asked
    // the questions a second time in the elevated window.
    if let Some((lang, ai, projects_dir, name)) = elevated_answers(&args) {
        let project_path = projects_dir.join(&name);
        run_first_install(&lang, ai, &projects_dir, &project_path, skip_install, true);
        return;
    }

    // Setup is an interactive wizard — it reads a menu from stdin. When stdin is
    // not a terminal (the classic case: `irm https://tina4.com/install.ps1 | iex`,
    // where the PowerShell host's stdin IS the consumed download pipe, already at
    // EOF), every prompt would silently default and then a UAC elevation would
    // fire from a non-interactive context and fail. That looked like setup
    // "dropping straight back to the prompt". Refuse cleanly instead, and tell the
    // user to run it in a real terminal.
    // --dry-run / --skip-install are non-interactive test paths and stay allowed.
    if !dry_run && !io::stdin().is_terminal() {
        println!();
        println!(
            "  {} Setup is interactive and needs a real terminal.",
            icon_info().blue()
        );
        println!(
            "  {} Open a new terminal and run:  {}",
            icon_play().green(),
            "tina4 setup".bold()
        );
        println!();
        // Exit 0 — this is expected guidance, not a failure. install.ps1 keys its
        // "Setup didn't finish" warning off a non-zero code; a clean exit avoids
        // that scary (and, here, misleading) message.
        return;
    }

    // First run configures the machine (language, AI tool, projects folder) and
    // remembers those choices. Every run after that is a fast path: it only asks
    // the project type + name and builds into the environment already set up.
    match load_config() {
        Some(cfg) => run_quick(dry_run, skip_install, cfg),
        None => run_first(dry_run, skip_install),
    }
}

/// First-time setup: ask everything, install the world, scaffold the first
/// project, and remember the projects folder + AI tool for next time. (The
/// elevated re-run is handled in `run()` before this is ever reached.)
fn run_first(dry_run: bool, skip_install: bool) {
    // Ask everything up front, in the user's current console — answering
    // questions needs no Administrator rights, so this must happen BEFORE any
    // elevation. (Elevating first would relaunch into a new window and leave
    // the original console looking like it just exited.)
    let lang = choose_language();
    let ai = choose_ai();
    let projects_dir = choose_projects_dir();
    let name = prompt("Name of your first project", "tina4example");
    let project_path = projects_dir.join(&name);

    if dry_run {
        print_plan(&lang, ai, &projects_dir, &name, &project_path, skip_install);
        return;
    }

    // Decide whether we even need Administrator. Python installs fully
    // admin-free (uv manages uv + Python + tina4python in the user profile), so
    // we DON'T elevate for it — elevating would relaunch into a separate admin
    // window and land everything in the admin's profile, which broke the run
    // step. Other languages still use Chocolatey (machine-wide → needs admin).
    let install_here = if skip_install {
        false
    } else if !needs_admin_to_install(&lang) {
        true // run installs right here in the user's console, no elevation
    } else {
        elevate_for_install(&lang, ai, &projects_dir, &name)
    };

    run_first_install(&lang, ai, &projects_dir, &project_path, skip_install || !install_here, false);
}

/// Does installing this language's runtime need Administrator on Windows?
/// Python does NOT — uv installs it (and itself, and tina4python) into the user
/// profile with no admin and no Chocolatey. The others currently install via
/// Chocolatey, which writes machine-wide and needs admin. (Non-Windows never
/// needs admin here — Homebrew/apt handle their own privileges.)
fn needs_admin_to_install(lang: &str) -> bool {
    if !console::is_windows() {
        return false;
    }
    !matches!(lang, "python")
}

/// The install + scaffold tail of first-time setup. Runs either in-process
/// (already admin / off Windows / --skip-install) or in the elevated re-run
/// (answers supplied via the environment).
fn run_first_install(
    lang: &str,
    ai: AiChoice,
    projects_dir: &Path,
    project_path: &Path,
    skip_install: bool,
    elevated: bool,
) {
    println!();
    println!("{} Setting up — this can take a few minutes...\n", icon_play().green());

    // macOS: Homebrew, git, and the PHP/Ruby/Node runtimes all need the Xcode
    // Command Line Tools; Python is toolchain-free (uv). If a non-Python language
    // is chosen and the tools are missing, ensure_macos_build_tools() launches
    // their installer and prints guidance — we then skip the installs but STILL
    // scaffold the project so it's ready, and the user re-runs once the tools
    // finish. (Returns true / no-op on non-macOS and for Python.)
    let clt_blocked = !skip_install
        && !matches!(lang, "python" | "py")
        && !install::ensure_macos_build_tools();

    if skip_install {
        println!(
            "  {} --skip-install: skipping runtime / git / AI / skills installs\n",
            icon_info().blue()
        );
    } else if clt_blocked {
        println!(
            "  {} Skipping installs until the Command Line Tools are ready — scaffolding your project so it's waiting for you.\n",
            icon_warn().yellow()
        );
    } else {
        // 1. Language runtime + package manager (Chocolatey/Homebrew/uv under the hood).
        install::run(lang);
        // 2. Git — version control + future updates.
        ensure_git();
        // 3. The tina4 AI skills, installed globally so whatever AI tool the
        //    user picks already knows how to build with tina4.
        install_skills_global(ai);
        // 4. The chosen AI tool.
        match ai {
            AiChoice::ClaudeDesktop => ensure_claude_desktop(),
            AiChoice::ClaudeCode => ensure_claude_code(),
            AiChoice::Codex => ensure_codex(),
            AiChoice::Cursor => {}
            AiChoice::All => {}
            AiChoice::None => {}
        }
        // Remember the environment so future `tina4 setup` runs are one-question
        // quick. Only after a real install — a --skip-install test machine isn't
        // actually configured.
        save_config(projects_dir, ai);
    }

    scaffold_into(projects_dir, project_path, lang, ai, elevated);
    pause_if_elevated(elevated);
}

/// Every run after the first: the machine is already set up, so only ask the
/// project type + name, then build into the saved projects folder. (The elevated
/// re-run is handled in `run()` before this is reached.)
fn run_quick(dry_run: bool, skip_install: bool, cfg: SetupConfig) {
    println!(
        "  {} Using your saved setup — projects in {}, AI: {}.",
        icon_info().blue(),
        cfg.projects_dir.display().to_string().cyan(),
        ai_label(cfg.ai)
    );
    println!("  {}", "(to change these, delete ~/.tina4/setup.conf and run setup again)".dimmed());
    println!();
    let lang = choose_language();
    let name = prompt("Name of your new project", "tina4example");
    let project_path = cfg.projects_dir.join(&name);
    let need_runtime = !runtime_present(&lang);

    if dry_run {
        println!();
        println!("  {} Dry run — no changes made. This would:", icon_info().blue());
        if need_runtime && !skip_install {
            println!("    - install the {} runtime (new language for this machine)", lang);
        }
        println!("    - scaffold '{}' at {}", name, project_path.display());
        println!("    - write a CLAUDE.md into the project");
        if cfg.ai == AiChoice::ClaudeDesktop {
            println!("    - open Claude Desktop");
        }
        println!();
        return;
    }

    // Only the elevation dance if we actually need to install a runtime the
    // machine doesn't have yet AND that runtime needs admin (Python via uv does
    // not — see needs_admin_to_install). Pass the answers so the elevated window
    // runs straight through without re-asking. Returns false (scaffold only) if
    // we need to install but couldn't get admin.
    let install_here = if need_runtime && !skip_install && needs_admin_to_install(&lang) {
        elevate_for_install(&lang, cfg.ai, &cfg.projects_dir, &name)
    } else {
        true
    };

    println!();
    println!("{} Building your project...\n", icon_play().green());

    if skip_install {
        println!("  {} --skip-install: skipping runtime install\n", icon_info().blue());
    } else if need_runtime && install_here {
        install::run(&lang);
    } else if need_runtime {
        // No admin: skip the install (guidance was already printed), scaffold anyway.
        println!("  {} Skipping the {} runtime install (no admin) — scaffolding your project.\n", icon_warn().yellow(), pretty_lang(&lang));
    } else {
        println!("  {} {} runtime already installed", icon_ok().green(), pretty_lang(&lang));
    }

    scaffold_into(&cfg.projects_dir, &project_path, &lang, cfg.ai, false);
    pause_if_elevated(false);
}

/// Shared tail for both modes: create the projects folder, scaffold the
/// project inside it, write its CLAUDE.md, open the tool, print next steps.
fn scaffold_into(projects_dir: &Path, project_path: &Path, lang: &str, ai: AiChoice, elevated: bool) {
    if let Err(e) = fs::create_dir_all(projects_dir) {
        eprintln!("  {} Could not create {}: {}", icon_warn().yellow(), projects_dir.display(), e);
    } else {
        println!("  {} Projects folder: {}", icon_ok().green(), projects_dir.display().to_string().cyan());
    }
    if let Err(e) = std::env::set_current_dir(projects_dir) {
        eprintln!("  {} Could not enter {}: {}", icon_warn().yellow(), projects_dir.display(), e);
    }
    // Setup owns the ending (CLAUDE.md, open IDE, next steps), so tell init not
    // to grab the terminal with its blocking "start server now?" prompt.
    std::env::set_var("TINA4_INIT_NO_SERVE", "1");
    init::run(Some(lang), project_path.file_name().and_then(|s| s.to_str()));

    match ai {
        AiChoice::Codex => write_project_codex_agents_md(project_path, lang),
        AiChoice::All => {
            write_project_codex_agents_md(project_path, lang);
            write_project_claude_md(project_path, lang, ai);
        }
        _ => write_project_claude_md(project_path, lang, ai),
    }
    let name = project_path.file_name().and_then(|s| s.to_str()).unwrap_or("app");
    write_project_mcp_json(project_path, lang, name);
    // The AI tool is opened from INSIDE whats_next, AFTER the "Start it now?"
    // prompt — opening it here (e.g. `open -a Claude`) steals terminal focus
    // before the user can answer, so the prompt goes unseen and nothing starts.
    whats_next(project_path, ai, elevated);
}

fn banner() {
    println!();
    println!("  {}", "Tina4 Setup".cyan());
    println!("  {}", "A few questions and you'll be building.".dimmed());
    println!();
}

/// Language menu with a short use-case tag next to each. Python is the
/// recommended default — it pairs best with AI.
fn choose_language() -> String {
    let opts = [
        ("python", "Python", "recommended · APIs, AI, data"),
        ("nodejs", "Node.js", "real-time apps, JS/TS teams"),
        ("php", "PHP", "classic web, shared hosting"),
        ("ruby", "Ruby", "rapid prototyping"),
    ];
    println!("  Which language do you want to build with?");
    for (i, (_, name, desc)) in opts.iter().enumerate() {
        let tag = if i == 0 { "  (default)".green().to_string() } else { String::new() };
        println!("    {}. {}{}  {}", i + 1, name, tag, format!("— {}", desc).dimmed());
    }
    let choice = prompt("Choose 1-4", "1");
    let idx = choice
        .trim()
        .parse::<usize>()
        .unwrap_or(1)
        .saturating_sub(1)
        .min(opts.len() - 1);
    opts[idx].0.to_string()
}

/// Which AI tool? — drives what we install and what we open at the end.
fn choose_ai() -> AiChoice {
    println!();
    println!("  Which AI tool do you want to build with?");
    println!("    1. {}  {}", "Claude Desktop".bold(), "(default) — the chat app; opens your project ready to build".dimmed());
    println!("    2. {}  {}", "Claude Code".bold(), "— AI in your terminal, opens a real coding session in your project".dimmed());
    println!("    3. {}  {}", "Codex".bold(), "— OpenAI coding agent in your terminal or desktop app".dimmed());
    println!("    4. {}  {}", "Cursor".bold(), "— AI-native IDE; installs skills into ~/.cursor/skills".dimmed());
    println!("    5. {}  {}", "All AI tools".bold(), "— Claude, Codex, and Cursor".dimmed());
    println!("    6. {}  {}", "Just my code editor".bold(), "— no AI".dimmed());
    let choice = prompt("Choose 1-6", "1");
    match choice.trim() {
        "2" => AiChoice::ClaudeCode,
        "3" => AiChoice::Codex,
        "4" => AiChoice::Cursor,
        "5" => AiChoice::All,
        "6" => AiChoice::None,
        _ => AiChoice::ClaudeDesktop,
    }
}

/// Where the user's projects live. Defaults to <home>/projects, created later
/// if it doesn't exist yet. Keeps every tina4 project in one tidy place.
fn choose_projects_dir() -> PathBuf {
    let default = home_dir().join("projects");
    println!();
    println!("  Where should your projects live? (created if it doesn't exist)");
    let entered = prompt("Projects folder", &default.display().to_string());
    expand_tilde(&entered)
}

fn print_plan(
    lang: &str,
    ai: AiChoice,
    projects_dir: &Path,
    name: &str,
    project_path: &Path,
    skip_install: bool,
) {
    println!();
    println!("  {} Dry run — no changes made. This setup would:", icon_info().blue());
    if skip_install {
        println!("    - {} skip all system installs", "(--skip-install)".dimmed());
    } else {
        if console::is_windows() {
            println!("    - install Chocolatey if missing (relaunching as Administrator)");
            println!("    - install the {} runtime + tools through it", lang);
        } else if matches!(lang, "python" | "py") {
            // Toolchain-free: uv installs uv + Python + tina4-python — no
            // Homebrew, and on macOS no Xcode Command Line Tools.
            println!("    - install Python + tina4-python via uv (no Homebrew / Xcode tools)");
        } else {
            if cfg!(target_os = "macos") {
                println!("    - ensure the Xcode Command Line Tools (install if missing)");
            }
            println!("    - install Homebrew if missing");
            println!("    - install the {} runtime + tools through it", lang);
        }
        println!("    - install Git if missing");
        match ai {
            AiChoice::ClaudeDesktop | AiChoice::ClaudeCode => {
                println!("    - install the tina4 AI skills globally (~/.claude/skills)")
            }
            AiChoice::Codex => println!("    - install the tina4 AI skills globally (~/.agents/skills)"),
            AiChoice::Cursor => println!("    - install the tina4 AI skills globally (~/.cursor/skills)"),
            AiChoice::All => println!("    - install the tina4 AI skills globally for Claude, Codex, and Cursor"),
            AiChoice::None => {}
        }
        match ai {
            AiChoice::ClaudeDesktop => println!("    - install Claude Desktop"),
            AiChoice::ClaudeCode => println!("    - install Claude Code"),
            AiChoice::Codex | AiChoice::Cursor | AiChoice::All | AiChoice::None => {}
        }
    }
    println!("    - create your projects folder: {}", projects_dir.display());
    println!("    - scaffold '{}' at {}", name, project_path.display());
    println!("    - write a CLAUDE.md into the project with instructions");
    match ai {
        AiChoice::ClaudeDesktop => println!("    - open Claude Desktop"),
        AiChoice::ClaudeCode => println!("    - show how to start Claude Code in the project"),
        AiChoice::Codex => println!("    - write AGENTS.md for Codex"),
        AiChoice::Cursor => println!("    - install Cursor skills globally (project uses CLAUDE.md)"),
        AiChoice::All => println!("    - write CLAUDE.md and AGENTS.md for all selected AI tools"),
        AiChoice::None => {}
    }
    println!();
}

/// `choco install` needs Administrator rights. If we're not elevated, relaunch
/// `tina4 setup` through UAC and hand off to that elevated instance — passing
/// the already-collected answers on ARGV so the elevated window runs straight
/// through without asking the questions again. No-op off Windows or when already
/// admin (caller continues in-process). The elevated re-run never reaches here —
/// run() intercepts it via --elevated before any menu or elevation.
///
/// Called AFTER the menu so the questions always run in the user's own console;
/// elevating first would relaunch into a new window and leave the original
/// looking like it just exited.
/// Returns true if system installs should run in THIS process (we're on
/// macOS/Linux, or already admin on Windows). Returns false when we're on
/// Windows without admin and couldn't elevate — the caller then scaffolds the
/// project but skips the system installs, with honest guidance, instead of
/// dead-ending. When elevation succeeds the elevated child takes over and this
/// process exits.
fn elevate_for_install(lang: &str, ai: AiChoice, projects_dir: &Path, name: &str) -> bool {
    if !console::is_windows() {
        return true;
    }
    if is_admin_windows() {
        return true;
    }

    println!();
    println!("  {} Setup needs Administrator rights to install software.", icon_info().blue());
    println!("  {} Approve the Windows prompt — a new window will finish the install.", icon_info().blue());
    println!();

    let Ok(exe) = std::env::current_exe() else { return false };
    // Start-Process … -Verb RunAs raises the UAC prompt and launches the elevated
    // child. Environment variables do NOT cross that boundary — the elevated
    // process gets a fresh environment — so the answers ride on ARGV instead
    // (the --elevated/--lang/--ai/--projects-dir/--name flags). The elevated
    // re-run reads them in elevated_answers() and skips the menu; --elevated
    // also guards against re-elevating.
    let q = |s: &str| s.replace('\'', "''");
    // PowerShell single-quoted argument list; each answer individually quoted.
    let arglist = format!(
        "'setup','--elevated','--lang','{lang}','--ai','{ai}','--projects-dir','{dir}','--name','{name}'",
        lang = q(lang),
        ai = ai_env(ai),
        dir = q(&projects_dir.display().to_string()),
        name = q(name),
    );
    let cmd = format!(
        "Start-Process -FilePath '{exe}' -ArgumentList {arglist} -Verb RunAs",
        exe = q(&exe.display().to_string()),
    );
    let launched = Command::new("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if launched {
        println!("  {} Continuing in the elevated window...", icon_ok().green());
        std::process::exit(0);
    }
    // No admin — either the UAC prompt was declined, or this is a standard
    // account with no admin credentials to satisfy it. Don't dead-end: carry on
    // and scaffold the project without the system installs, and tell the user
    // exactly how to get the runtime WITHOUT admin (the Python path needs none).
    println!(
        "  {} No Administrator rights — skipping the system installs and just \
         setting up your project.",
        icon_warn().yellow()
    );
    println!(
        "  {} To get the {} runtime without admin:",
        icon_info().blue(),
        pretty_lang(lang)
    );
    match lang {
        // uv installs to the user profile (no admin) and can even fetch Python
        // itself — so a Python project needs no Administrator at all.
        "python" => {
            println!("       powershell -c \"irm https://astral.sh/uv/install.ps1 | iex\"");
            println!("       uv python install 3.12");
        }
        "nodejs" => {
            println!("       Install Node for your user from https://nodejs.org (per-user, no admin),");
            println!("       or use a user-level manager like fnm / nvm-windows.");
        }
        "php" => {
            println!("       Download the PHP zip from https://windows.php.net/download/ and add it to your PATH,");
            println!("       or ask an administrator to run: choco install php");
        }
        "ruby" => {
            println!("       Install RubyInstaller from https://rubyinstaller.org (per-user, no admin).");
        }
        _ => {}
    }
    println!(
        "  {} Claude Code (terminal) and Claude Desktop both install per-user, no admin needed.",
        icon_info().blue()
    );
    println!();
    false
}

/// The elevated re-run receives the menu answers on ARGV (env vars don't survive
/// the UAC boundary). Returns them only when this IS the elevated re-run
/// (`--elevated` plus the answer flags present).
fn elevated_answers(args: &SetupArgs) -> Option<(String, AiChoice, PathBuf, String)> {
    if !args.elevated {
        return None;
    }
    let lang = args.lang.clone()?;
    let ai = match args.ai.as_deref()? {
        "code" => AiChoice::ClaudeCode,
        "codex" => AiChoice::Codex,
        "cursor" => AiChoice::Cursor,
        "all" => AiChoice::All,
        "none" => AiChoice::None,
        _ => AiChoice::ClaudeDesktop,
    };
    let dir = PathBuf::from(args.projects_dir.clone()?);
    let name = args.name.clone()?;
    Some((lang, ai, dir, name))
}

/// Stable env token for an AI choice (round-trips through `elevated_answers`).
fn ai_env(ai: AiChoice) -> &'static str {
    match ai {
        AiChoice::ClaudeDesktop => "desktop",
        AiChoice::ClaudeCode => "code",
        AiChoice::Codex => "codex",
        AiChoice::Cursor => "cursor",
        AiChoice::All => "all",
        AiChoice::None => "none",
    }
}

/// In the elevated re-run the new window closes the instant setup returns —
/// hold it open so the user can read the result.
fn pause_if_elevated(elevated: bool) {
    if elevated {
        let _ = prompt("\n  Setup finished — press Enter to close this window", "");
    }
}

/// True only in an elevated shell. `net session` needs admin and exits non-zero
/// otherwise — a dependency-free way to detect elevation.
fn is_admin_windows() -> bool {
    Command::new("net")
        .args(["session"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_git() {
    if which::which("git").is_ok() {
        println!("  {} Git already installed", icon_ok().green());
        return;
    }
    println!("  {} Installing Git...", icon_play().green());
    let ok = if console::is_windows() {
        run_status("choco", &["install", "git", "-y"])
    } else if which::which("brew").is_ok() {
        run_status("brew", &["install", "git"])
    } else {
        run_status("sh", &["-c", "sudo apt-get install -y git || sudo dnf install -y git"])
    };
    if !ok {
        println!("  {} Git install skipped — install it later if you want version control", icon_warn().yellow());
    }
}

fn ensure_claude_desktop() {
    // Claude Desktop isn't a CLI on PATH, so `which` can't see it — check its
    // known install location and skip the (re)install if it's already there.
    // Without this, `choco install` / `brew install --cask` would reinstall on
    // top of an existing app on every `tina4 setup` run.
    if claude_desktop_installed() {
        println!("  {} Claude Desktop already installed", icon_ok().green());
        return;
    }
    if console::is_windows() {
        println!("  {} Installing Claude Desktop...", icon_play().green());
        run_status("choco", &["install", "claude", "-y"]);
    } else if which::which("brew").is_ok() {
        println!("  {} Installing Claude Desktop...", icon_play().green());
        run_status("brew", &["install", "--cask", "claude"]);
    } else {
        println!("  {} Download Claude Desktop: https://claude.ai/download", icon_info().blue());
    }
    // Claude Desktop is installed. Connecting it to a project's live MCP
    // endpoint (`/__dev/mcp`, served by `tina4 serve`) is a manual step in
    // Desktop's connector settings — we don't write Desktop's global config
    // from here because the connector schema varies and Windows stores it
    // under an MSIX-virtualized path. (Claude Code connects to the same URL
    // directly.) This is intentionally NOT auto-wired, not a pending TODO.
}

/// Best-effort detection of an existing Claude Desktop install. Checks the
/// default install locations per platform — the app is a GUI, not a PATH binary,
/// so there's nothing for `which` to find.
fn claude_desktop_installed() -> bool {
    if console::is_windows() {
        // The AnthropicClaude dir alone is enough of a signal that it's
        // installed (claude_desktop_target may still be resolving the exact
        // launch target). Check both so a present-but-unusual layout still
        // counts as installed and we don't reinstall over it.
        claude_desktop_target().is_some()
            || std::env::var("LOCALAPPDATA")
                .map(|l| Path::new(&l).join("AnthropicClaude").exists())
                .unwrap_or(false)
    } else if cfg!(target_os = "macos") {
        Path::new("/Applications/Claude.app").exists()
    } else {
        // No official Linux build; nothing reliable to detect.
        false
    }
}

/// Resolve a launchable Claude Desktop target on Windows — an .exe or a Start
/// Menu .lnk shortcut. Claude Desktop is a GUI app, NOT on PATH as `claude`, so
/// `start claude` fails with "Windows cannot find 'claude'". Different installers
/// drop it in different places (the official per-user installer, the Chocolatey
/// package, MSIX), so we check every known location and also the Start Menu
/// shortcut, which is location-independent and present for any normal install.
/// Returns the first target that EXISTS (so callers can `start` it without
/// risking a missing-file dialog), or None when nothing is found.
fn claude_desktop_target() -> Option<PathBuf> {
    if !console::is_windows() {
        return None;
    }
    let env = |k: &str| std::env::var(k).ok().map(PathBuf::from);
    let local = env("LOCALAPPDATA");
    let appdata = env("APPDATA");
    let programdata = env("PROGRAMDATA");

    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1. Official per-user install: %LOCALAPPDATA%\AnthropicClaude\claude.exe
    //    plus newest versioned app-*/claude.exe.
    if let Some(local) = &local {
        let root = local.join("AnthropicClaude");
        candidates.push(root.join("claude.exe"));
        if let Ok(rd) = std::fs::read_dir(&root) {
            let mut apps: Vec<PathBuf> = rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|s| s.to_str())
                            .map(|n| n.starts_with("app-"))
                            .unwrap_or(false)
                })
                .collect();
            apps.sort();
            if let Some(newest) = apps.pop() {
                candidates.push(newest.join("claude.exe"));
            }
        }
        // 2. Some installers use %LOCALAPPDATA%\Programs\claude\Claude.exe
        candidates.push(local.join("Programs").join("claude").join("Claude.exe"));
    }

    // 3. Start Menu shortcuts — location-independent; created by every normal
    //    install. `start ""  <lnk>` launches whatever the shortcut points to.
    for base in [appdata.as_ref(), programdata.as_ref()].into_iter().flatten() {
        let sm = base
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        candidates.push(sm.join("Claude.lnk"));
        candidates.push(sm.join("Claude").join("Claude.lnk"));
        candidates.push(sm.join("Anthropic").join("Claude.lnk"));
    }

    candidates.into_iter().find(|p| p.exists())
}

/// Is Claude Code already on this machine? More thorough than a bare
/// `which("claude")`: in a freshly-elevated Windows window (or right after a
/// native install) the `claude` shim lives in ~/.local/bin or an npm-global dir
/// that isn't on PATH yet, so `which` alone reports a false negative. Splice
/// ~/.local/bin onto PATH first, then check PATH AND the known install
/// locations. This stops `tina4 setup` from reinstalling Claude Code on top of
/// an existing install (which clobbers the user's running sessions).
fn claude_code_installed() -> bool {
    refresh_local_bin_path();
    if which::which("claude").is_ok() {
        return true;
    }
    // Native installer drops the launcher in ~/.local/bin.
    let local_bin = home_dir().join(".local").join("bin");
    let candidates: &[&str] = if console::is_windows() {
        &["claude.exe", "claude.cmd", "claude.ps1", "claude"]
    } else {
        &["claude"]
    };
    candidates.iter().any(|c| local_bin.join(c).exists())
}

fn ensure_claude_code() {
    if claude_code_installed() {
        println!("  {} Claude Code already installed", icon_ok().green());
        return;
    }
    println!("  {} Installing Claude Code (no Node required)...", icon_play().green());
    // Native installer — does NOT depend on Node.js. (npm install -g is only
    // for users who already have Node and prefer it.)
    if console::is_windows() {
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", "irm https://claude.ai/install.ps1 | iex"])
            .status();
    } else {
        let _ = Command::new("sh")
            .args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"])
            .status();
    }
    refresh_local_bin_path();
    if which::which("claude").is_ok() {
        println!("  {} Claude Code installed", icon_ok().green());
    } else {
        println!(
            "  {} Claude Code installed — open a new terminal to use `claude` (docs: {})",
            icon_info().blue(),
            "https://docs.claude.com/claude-code".cyan()
        );
    }
}

/// The native Claude Code installer (and uv, and many CLI tools) drops binaries
/// into ~/.local/bin, which a running process won't have on PATH yet. Splice it
/// in so `which::which("claude")` resolves without opening a new shell.
fn refresh_local_bin_path() {
    let bin = home_dir().join(".local").join("bin");
    if !bin.exists() {
        return;
    }
    let sep = if console::is_windows() { ';' } else { ':' };
    let current = std::env::var("PATH").unwrap_or_default();
    let bin_s = bin.display().to_string();
    if !current.split(sep).any(|p| p == bin_s) {
        std::env::set_var("PATH", format!("{bin_s}{sep}{current}"));
    }
}

/// Install the tina4 AI skills (tina4-developer + tina4-js) into ~/.claude/skills
/// by running the hosted installer script — the same canonical source the
/// standalone one-liner uses, so there's a single source of truth.
fn install_skills_global(ai: AiChoice) {
    let target = match ai {
        AiChoice::ClaudeDesktop | AiChoice::ClaudeCode => "claude",
        AiChoice::Codex => "codex",
        AiChoice::Cursor => "cursor",
        AiChoice::All => "all",
        AiChoice::None => return,
    };
    install_skills_target(target);
}

/// Install skills without exposing installer implementation details to the
/// user. `tina4 skills codex` is the normal refresh path; the hosted script
/// still accepts environment variables for automation.
pub fn install_skills(target: &str) -> bool {
    match target {
        "claude" | "codex" | "cursor" | "all" => install_skills_target(target),
        _ => {
            eprintln!(
                "  {} Choose one of: tina4 skills claude, tina4 skills codex, tina4 skills cursor, or tina4 skills all.",
                icon_warn().yellow()
            );
            false
        }
    }
}

/// The human-friendly global skills flow. It is intentionally separate from
/// setup: existing developers can refresh skills without creating a project.
pub fn install_skills_interactive() -> bool {
    println!();
    println!("  Tina4 AI Skills");
    println!("    1. Claude");
    println!("    2. Codex");
    println!("    3. Cursor");
    println!("    4. All three");
    let choice = prompt("Choose 1-4", "4");
    install_skills(skills_target_from_choice(&choice))
}

fn skills_target_from_choice(choice: &str) -> &'static str {
    match choice.trim() {
        "1" | "claude" => "claude",
        "2" | "codex" => "codex",
        "3" | "cursor" => "cursor",
        _ => "all",
    }
}

fn install_skills_target(target: &str) -> bool {
    println!("  {} Installing tina4 AI skills for {}...", icon_play().green(), target);
    let ok = if console::is_windows() {
        run_status(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                &format!("$env:TINA4_SKILLS_TARGET='{target}'; irm https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.ps1 | iex"),
            ],
        )
    } else {
        run_status(
            "sh",
            &["-c", &format!("curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET={target} sh")],
        )
    };
    if !ok {
        println!(
            "  {} Skills install skipped — run later: {}",
            icon_warn().yellow(),
            "tina4 ai".cyan()
        );
    }
    ok
}

fn ensure_codex() {
    if which::which("codex").is_ok() {
        println!("  {} Codex already installed", icon_ok().green());
    } else {
        println!("  {} Install Codex, then run `tina4 setup` again to launch it from new projects.", icon_info().blue());
    }
}

/// Write a per-project `.mcp.json` that wires Claude Code to the project's live
/// MCP tools (`/__dev/mcp`, served by `tina4 serve`). Won't clobber an existing
/// one. The dev-server port is per-language (python:7146, php:7145, ruby:7147,
/// nodejs:7148).
fn write_project_mcp_json(project_path: &Path, lang: &str, name: &str) {
    let file = project_path.join(".mcp.json");
    if file.exists() {
        println!("  {} .mcp.json already present — left as-is", icon_info().blue());
        return;
    }
    let port = match lang {
        "php" => 7145,
        "ruby" => 7147,
        "nodejs" => 7148,
        _ => 7146, // python + default
    };
    // Build the literal JSON without format! so the braces don't need escaping.
    let url = "http://localhost:".to_string() + &port.to_string() + "/__dev/mcp/sse";
    let mut content = String::new();
    content.push_str("{\n");
    content.push_str("  \"mcpServers\": {\n");
    content.push_str("    \"");
    content.push_str(name);
    content.push_str("\": {\n");
    content.push_str("      \"type\": \"sse\",\n");
    content.push_str("      \"url\": \"");
    content.push_str(&url);
    content.push_str("\"\n");
    content.push_str("    }\n");
    content.push_str("  }\n");
    content.push_str("}\n");
    match fs::write(&file, content) {
        Ok(_) => println!(
            "  {} Wrote .mcp.json (Claude Code → live tina4 tools at /__dev/mcp)",
            icon_ok().green()
        ),
        Err(e) => eprintln!("  {} Could not write .mcp.json: {}", icon_warn().yellow(), e),
    }
}

/// Write concise Codex project instructions without overwriting existing guidance.
fn write_project_codex_agents_md(project_path: &Path, lang: &str) {
    let file = project_path.join("AGENTS.md");
    if file.exists() {
        println!("  {} AGENTS.md already present — left as-is", icon_info().blue());
        return;
    }
    let content = format!(r#"# Tina4 {lang} project instructions

Use the installed Tina4 skills in `~/.agents/skills/` as the framework API source of truth. Follow Tina4 conventions: routes belong in `src/routes/`, models in `src/orm/`, templates in `src/templates/`, and migrations in `src/migrations/`.

Before changing code, inspect the matching language skill. After changes, run the framework test command and report its result. Keep the core dependency-free and use parameterized SQL for raw queries.
"#);
    match fs::write(&file, content) {
        Ok(_) => println!("  {} Wrote AGENTS.md (Codex project instructions)", icon_ok().green()),
        Err(e) => eprintln!("  {} Could not write AGENTS.md: {}", icon_warn().yellow(), e),
    }
}

/// Write a project-level CLAUDE.md so the chosen AI tool has clear, accurate
/// instructions for working in THIS tina4 project. Won't clobber an existing one.
fn write_project_claude_md(project_path: &Path, lang: &str, ai: AiChoice) {
    let file = project_path.join("CLAUDE.md");
    if file.exists() {
        println!("  {} CLAUDE.md already present — left as-is", icon_info().blue());
        return;
    }
    let name = project_path.file_name().and_then(|s| s.to_str()).unwrap_or("app");
    let ai_line = match ai {
        AiChoice::ClaudeCode => "You're working in **Claude Code** — you have this project's CLAUDE.md and (via .mcp.json) its live `/__dev/mcp` tools.",
        AiChoice::ClaudeDesktop => "You're working in **Claude Desktop**.",
        AiChoice::Codex => "You're working in **Codex**.",
        AiChoice::Cursor => "You're working in **Cursor** — use the Tina4 skills in `~/.cursor/skills/` (and this project's CLAUDE.md).",
        AiChoice::All => "You're working with **Claude, Codex, and Cursor** — use the installed Tina4 skills for the tool you opened.",
        AiChoice::None => "This project is set up for AI-assisted development.",
    };

    // Built with String + push_str of RAW literals because the route / Frond
    // snippets contain `{` `}` `{{ }}` `{% %}` that format! would try to parse.
    let mut s = String::new();

    // Title + ai_line (no literal braces — safe to format).
    s.push_str(&format!("# {} — Tina4 {} project\n\n", name, pretty_lang(lang)));
    s.push_str(ai_line);
    s.push('\n');

    s.push_str(r#"
**Tina4 v3** — *The Intelligent Native Application 4ramework*. Built for AI:
zero third-party dependencies, convention over configuration, one small
consistent API. Use the framework's built-ins (routing, ORM, migrations, Frond
templates, auth/JWT, queues, cache, sessions, WebSockets, GraphQL) before any
library or hand-rolled code.

## Source of truth — check before you guess

1. **Skills** in `~/.claude/skills/` — **tina4-developer** (+ **tina4-js** for
   the reactive frontend). These document the real API surface.
2. **https://tina4.com** — docs + the **Ask Tina4** RAG box (ask it any
   framework question; it answers from the live corpus).
3. **`/__dev/mcp`** live tools (wired via the `.mcp.json` in this folder) —
   query for real routes, models, and signatures when the dev server is running.

## Environment (.env)

```bash
TINA4_DEBUG=true                              # dev mode: hot-reload, error overlay, /__dev
TINA4_SECRET=change-me                        # JWT signing secret
TINA4_DATABASE_URL=sqlite:///data/app.db      # driver://host:port/db
TINA4_DATABASE_USERNAME=                       # db user (blank for sqlite)
TINA4_DATABASE_PASSWORD=                       # db password
TINA4_LOG_LEVEL=INFO                          # ALL | DEBUG | INFO | WARNING | ERROR
TINA4_API_KEY=                                 # optional static bearer token
TINA4_CACHE_BACKEND=memory                    # memory | file | redis | valkey | memcached | mongodb | database
TINA4_SESSION_BACKEND=file                    # session store
TINA4_NO_BROWSER=false                        # set true to never auto-open the browser
```

"#);

    // ── Database & drivers (per language) ──────────────────────────
    s.push_str("## Database & drivers\n\n");
    s.push_str("SQLite works out of the box:\n\n");
    s.push_str("```bash\n");
    match lang {
        "php" => s.push_str("TINA4_DATABASE_URL=sqlite:///data/app.db\n"),
        "ruby" => s.push_str("TINA4_DATABASE_URL=sqlite:///data/app.db\n"),
        "nodejs" => s.push_str("TINA4_DATABASE_URL=sqlite://./data/app.db   # SQLite is built in via node:sqlite — no install\n"),
        _ => s.push_str("TINA4_DATABASE_URL=sqlite:///data/app.db   # three slashes = relative to cwd\n"),
    }
    s.push_str("```\n\n");
    s.push_str("Connection URLs are `driver://host:port/database` (sqlite / postgres / postgresql / mysql / mssql / firebird).\n\n");
    s.push_str("Add a Postgres or MySQL driver:\n\n");
    match lang {
        "php" => {
            s.push_str("- **Postgres**: enable the PDO PostgreSQL extension (no Composer pkg — Tina4 v3 has zero runtime deps). macOS `brew install php` ships it, or `pecl install pdo_pgsql`; Debian/Ubuntu `sudo apt-get install php-pgsql`. Verify: `php -m | grep pdo_pgsql`.\n");
            s.push_str("- **MySQL**: enable the PDO MySQL extension (`sudo apt-get install php-mysql`, or bundled with Homebrew PHP / `pecl install pdo_mysql`). Verify: `php -m | grep pdo_mysql`.\n\n");
            s.push_str("```bash\nTINA4_DATABASE_URL=postgres://user:pass@localhost:5432/mydb\n```\n\n");
        }
        "ruby" => {
            s.push_str("```bash\nbundle add pg        # Postgres\nbundle add mysql2    # MySQL\n```\n\n");
            s.push_str("```bash\nTINA4_DATABASE_URL=postgres://localhost:5432/mydb   # + TINA4_DATABASE_USERNAME / TINA4_DATABASE_PASSWORD\n```\n\n");
        }
        "nodejs" => {
            s.push_str("```bash\nnpm i pg        # Postgres\nnpm i mysql2    # MySQL\n```\n\n");
            s.push_str("```bash\nTINA4_DATABASE_URL=postgres://localhost:5432/mydb\n```\n\n");
        }
        _ => {
            s.push_str("```bash\nuv add psycopg2-binary           # Postgres\nuv add mysql-connector-python    # MySQL\n```\n\n");
            s.push_str("```bash\nTINA4_DATABASE_URL=postgresql://localhost:5432/mydb   # + TINA4_DATABASE_USERNAME / TINA4_DATABASE_PASSWORD\n```\n\n");
        }
    }

    // ── Add a route (per language) ─────────────────────────────────
    s.push_str("## Add a route\n\n");
    match lang {
        "php" => {
            s.push_str("`src/routes/hello.php` (auto-discovered; one resource/verb per file):\n\n");
            s.push_str("```php\n");
            s.push_str(r#"<?php
\Tina4\Router::get("/hello", function ($request, $response) {
    return $response->json(["message" => "Hello from Tina4"]);
});
"#);
            s.push_str("```\n\n");
        }
        "ruby" => {
            s.push_str("`src/routes/hello.rb` (auto-discovered):\n\n");
            s.push_str("```ruby\n");
            s.push_str(r#"require "tina4"

Tina4.get "/hello" do |request, response|
  response.json({ message: "Hello from Tina4" }, Tina4::HTTP_OK)
end
"#);
            s.push_str("```\n\n");
        }
        "nodejs" => {
            s.push_str("`src/routes/hello/get.ts` — **file-based**: the directory is the URL path, the FILENAME is the HTTP method (`get.ts` = `GET /hello`):\n\n");
            s.push_str("```ts\n");
            s.push_str(r#"import type { Tina4Request, Tina4Response } from "@tina4/core";

export default async function (req: Tina4Request, res: Tina4Response) {
  return res.json({ message: "Hello from Tina4" });
}
"#);
            s.push_str("```\n\n");
        }
        _ => {
            s.push_str("`src/routes/hello.py` (auto-discovered; one resource per file):\n\n");
            s.push_str("```python\n");
            s.push_str(r#"from tina4_python.core.router import get


@get("/hello")
async def hello(request, response):
    return response({"message": "Hello from Tina4"})
"#);
            s.push_str("```\n\n");
        }
    }

    // ── Templates (Frond) ──────────────────────────────────────────
    s.push_str("## Templates (Frond)\n\n");
    s.push_str("Frond is the built-in zero-dep Twig-compatible engine; templates live in `src/templates/` (`.twig`). Common syntax:\n\n");
    s.push_str("```twig\n");
    s.push_str(r#"{% extends "base.twig" %}
{% block content %}
  <h1>{{ title }}</h1>
  <ul>
    {% for x in items %}
      <li>{{ x.name | upper }}</li>
    {% endfor %}
  </ul>
{% endblock %}
"#);
    s.push_str("```\n\n");
    s.push_str("Render it from a route:\n\n");
    match lang {
        "php" => s.push_str("```php\nreturn $response->render(\"dashboard.twig\", [\"title\" => \"Dashboard\"]);\n```\n\n"),
        "ruby" => s.push_str("```ruby\nhtml = Tina4::Template.render(\"index.twig\", { title: \"Home\" })\nresponse.html(html)\n```\n\n"),
        "nodejs" => s.push_str("```ts\nreturn res.render(\"page.twig\", { title: \"Home\" });\n```\n\n"),
        _ => s.push_str("```python\nreturn response.render(\"hello.twig\", {\"name\": \"Tina4\"})\n```\n\n"),
    }

    // ── How to run ─────────────────────────────────────────────────
    s.push_str(r#"## How to run

```bash
tina4 serve
```

Dev server: watches files, hot-reloads, opens the app + the `/__dev` dashboard.

> Dev runs two ports: the **base** port hot-reloads (for you); **base+1000** is
> stable and does NOT reload — use that one when an AI is driving the browser so
> a reload doesn't interrupt it.

## Where things go

| You want to…            | Put it in…                     | Make it with                                           |
|-------------------------|--------------------------------|--------------------------------------------------------|
| Add a page or API route | `src/routes/`                  | `tina4 generate route <name>`                          |
| Add a database model    | `src/orm/`                     | `tina4 generate model <Name>`                          |
| Change the schema       | `migrations/`                  | `tina4 generate migration <name>` then `tina4 migrate` |
| Add a page template     | `src/templates/`               | Frond (`.twig`)                                        |
| Frontend behaviour      | `tina4-js`                     | tina4-js signals + html templates                      |

## Golden rules

- **Use built-ins first** — Tina4 is zero-dep; reach for the framework before any library or hand-rolled code.
- **Don't guess API names** — check the skills / **Ask Tina4** at https://tina4.com / the live `/__dev/mcp` tools.
- Routes return data; **`response()`** (called, not `response.json`) auto-serializes models, lists, and `DatabaseResult` to JSON.
- **One resource per file** in `src/routes/` and `src/orm/`.
- All schema changes go through migrations (`tina4 generate migration` → `tina4 migrate`) — never raw DDL in routes.
- No inline styles / no hardcoded hex — use tina4-css classes + SCSS in `src/scss/`.
- Env comes from `.env`; `TINA4_DEBUG=true` in dev.
- All links point to **https://tina4.com**.
"#);

    // Per-language gotcha (one sharp line).
    match lang {
        "php" => s.push_str("- **PHP gotcha:** `return $response(...)` (callable) and `$response->json(...)` both emit JSON and auto-serialize models/arrays/`DatabaseResult`. Route files of pure `Router::*()` calls hot-reload; files declaring top-level functions/classes need a server restart.\n"),
        "ruby" => s.push_str("- **Ruby gotcha:** the handler block is `|request, response|`; pass an HTTP status like `Tina4::HTTP_OK` to `response.json`. The `sqlite3` gem ships by default; `pg`/`mysql2` are add-ons.\n"),
        "nodejs" => s.push_str("- **Node.js gotcha:** the route filename = HTTP method (`get.ts`/`post.ts`/…); dirs map to the path (`[id]` → `{id}`). Use `.js` extensions in import paths. `res.json(model | model[] | DatabaseResult)` auto-serializes.\n"),
        _ => s.push_str("- **Python gotcha:** route decorators (`@get`/`@post`/…) must be INNERMOST (closest to `def`); `@noauth`/`@secured`/`@description` go above. GET is public; POST/PUT/PATCH/DELETE need auth unless `@noauth()`. Use `response(...)`, not `response.json()`.\n"),
    }

    // ── First prompt ───────────────────────────────────────────────
    s.push_str("\n## A good first prompt\n\n");
    s.push_str("> ");
    s.push_str(FIRST_PROMPT);
    s.push('\n');

    match fs::write(&file, s) {
        Ok(_) => println!("  {} Wrote {}", icon_ok().green(), "CLAUDE.md".cyan()),
        Err(e) => eprintln!("  {} Could not write CLAUDE.md: {}", icon_warn().yellow(), e),
    }
}

/// Best-effort: bring Claude Desktop to the foreground. Never fatal —
/// whats_next() always prints the manual command too. Only handles
/// AiChoice::ClaudeDesktop; Claude Code opens its own session inline in
/// whats_next, and AiChoice::None has nothing to open.
fn open_ide(ai: AiChoice) {
    if ai != AiChoice::ClaudeDesktop {
        return;
    }
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").args(["-a", "Claude"]).status();
    } else if console::is_windows() {
        // Claude Desktop is NOT on PATH as `claude` (it's a GUI app), so
        // `start "" claude` popped "Windows cannot find 'claude'". Launch a
        // RESOLVED target that we've confirmed exists (an .exe or Start Menu
        // .lnk) so `start` never raises a missing-file dialog. If nothing is
        // found, say so plainly and skip — never error out.
        match claude_desktop_target() {
            Some(target) => {
                let _ = Command::new("cmd")
                    .args(["/C", "start", ""])
                    .arg(target)
                    .status();
            }
            None => {
                println!(
                    "  {} Couldn't find Claude Desktop to open — launch it from the Start menu.",
                    icon_info().blue()
                );
            }
        }
    }
}

fn whats_next(project_path: &Path, ai: AiChoice, elevated: bool) {
    let p = project_path.display();
    println!();
    println!("  {} Your project is ready: {}", icon_ok().green(), p.to_string().cyan());
    println!();
    // Always print the commands first — the fallback if anything below is
    // skipped or interrupted.
    println!("  Start it any time:");
    println!("    cd {}", p);
    println!("    tina4 serve        {}", "# opens your app in the browser".dimmed());
    println!();

    // In the elevated Windows install window the user serves from their own
    // console; the printed commands above are the fallback. Nothing to open.
    if elevated {
        return;
    }

    match ai {
        AiChoice::ClaudeCode => {
            // The hands-off path: open a real Claude Code session in the
            // project, seeded with the first prompt. The session owns the
            // terminal and can run `tina4 serve` itself (CLAUDE.md tells it),
            // so we do NOT also prompt-to-serve here.
            println!(
                "  {} Opening Claude Code in your project (it has your CLAUDE.md + live tools)...",
                icon_play().green()
            );
            // Splice ~/.local/bin onto PATH so a just-installed claude resolves
            // without opening a new shell (mirrors claude_code_installed).
            refresh_local_bin_path();
            match which::which("claude") {
                Ok(claude) => {
                    // Launch the resolved binary so this works on macOS AND
                    // Windows. On Windows `claude` is a .cmd/.ps1 shim that
                    // Command::new cannot spawn directly — run it through
                    // cmd.exe with its full resolved path.
                    let status = if console::is_windows() {
                        Command::new("cmd")
                            .arg("/C")
                            .arg(&claude)
                            .arg(FIRST_PROMPT)
                            .current_dir(project_path)
                            .status()
                    } else {
                        Command::new(&claude)
                            .arg(FIRST_PROMPT)
                            .current_dir(project_path)
                            .status()
                    };
                    if status.is_err() {
                        println!("  {} Couldn't launch Claude Code automatically.", icon_info().blue());
                        println!("  Start a session:  {} && {}", format!("cd {}", p).cyan(), "claude".cyan());
                        println!("  First prompt: {}", FIRST_PROMPT);
                    }
                }
                Err(_) => {
                    println!("  Start a session:  {} && {}", format!("cd {}", p).cyan(), "claude".cyan());
                    println!("  First prompt: {}", FIRST_PROMPT);
                }
            }
        }
        AiChoice::ClaudeDesktop | AiChoice::Codex | AiChoice::Cursor | AiChoice::All | AiChoice::None => {
            // Offer to launch it right now — cd into the project and
            // `tina4 serve`, which opens the browser on the running app.
            let ans = prompt("Start it now and open it in your browser?", "y");
            // Open the GUI tool AFTER reading the answer — opening it before
            // the prompt steals terminal focus so the prompt goes unseen.
            open_ide(ai);
            if matches!(ans.trim().to_lowercase().as_str(), "" | "y" | "yes") {
                let label = project_path.file_name().and_then(|s| s.to_str()).unwrap_or("your app");
                println!();
                println!(
                    "  {} Starting {} — your browser will open. Press Ctrl+C to stop.",
                    icon_play().green(),
                    label.cyan()
                );
                println!();
                // Re-exec ourselves as `tina4 serve` inside the project so it
                // picks up app.py/index.php/app.rb/app.ts and serves THIS project.
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tina4"));
                let _ = Command::new(exe).arg("serve").current_dir(project_path).status();
            } else {
                println!("  {} No problem — run {} when you're ready.", icon_info().blue(), "tina4 serve".cyan());
            }
        }
    }
}

// ── config (zero-dep key=value file) ─────────────────────────────

fn config_path() -> PathBuf {
    home_dir().join(".tina4").join("setup.conf")
}

/// Read the configured projects folder from ~/.tina4/setup.conf, if it exists.
/// Used by `tina4 serve <name>` to resolve a project by name when it isn't in
/// the current folder. Returns None when there's no config or no projects_dir
/// line.
pub fn configured_projects_dir() -> Option<PathBuf> {
    let text = fs::read_to_string(config_path()).ok()?;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("projects_dir=") {
            return Some(PathBuf::from(v.trim()));
        }
    }
    None
}

/// Load the remembered setup, if the first run has happened. Returns None when
/// there's no config (→ first run) or the projects folder line is missing.
fn load_config() -> Option<SetupConfig> {
    let text = fs::read_to_string(config_path()).ok()?;
    let mut projects_dir: Option<PathBuf> = None;
    let mut ai = AiChoice::None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("projects_dir=") {
            projects_dir = Some(PathBuf::from(v.trim()));
        } else if let Some(v) = line.strip_prefix("ai=") {
            ai = ai_from_str(v.trim());
        }
    }
    Some(SetupConfig { projects_dir: projects_dir?, ai })
}

fn save_config(projects_dir: &Path, ai: AiChoice) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let body = format!("projects_dir={}\nai={}\n", projects_dir.display(), ai_to_str(ai));
    let _ = fs::write(path, body);
}

fn ai_to_str(ai: AiChoice) -> &'static str {
    match ai {
        AiChoice::ClaudeDesktop => "claude-desktop",
        AiChoice::ClaudeCode => "claude-code",
        AiChoice::Codex => "codex",
        AiChoice::Cursor => "cursor",
        AiChoice::All => "all",
        AiChoice::None => "none",
    }
}

fn ai_from_str(s: &str) -> AiChoice {
    match s {
        "claude-desktop" => AiChoice::ClaudeDesktop,
        "claude-code" => AiChoice::ClaudeCode,
        "codex" => AiChoice::Codex,
        "cursor" => AiChoice::Cursor,
        "all" => AiChoice::All,
        _ => AiChoice::None,
    }
}

fn ai_label(ai: AiChoice) -> &'static str {
    match ai {
        AiChoice::ClaudeDesktop => "Claude Desktop",
        AiChoice::ClaudeCode => "Claude Code",
        AiChoice::Codex => "Codex",
        AiChoice::Cursor => "Cursor",
        AiChoice::All => "All AI tools",
        AiChoice::None => "code editor only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_menu_maps_each_supported_target() {
        assert_eq!(skills_target_from_choice("1"), "claude");
        assert_eq!(skills_target_from_choice("codex"), "codex");
        assert_eq!(skills_target_from_choice("3"), "cursor");
        assert_eq!(skills_target_from_choice("4"), "all");
        assert_eq!(skills_target_from_choice("unexpected"), "all");
    }

    #[test]
    fn all_ai_choice_round_trips_in_setup_config() {
        assert_eq!(ai_from_str(ai_to_str(AiChoice::All)), AiChoice::All);
    }
}

/// Is the runtime for this language already on the machine? Lets quick runs
/// skip the package-manager / elevation dance unless a new language was picked.
fn runtime_present(lang: &str) -> bool {
    match lang {
        "python" => which::which("python3").is_ok() || which::which("python").is_ok(),
        "nodejs" => which::which("node").is_ok(),
        "php" => which::which("php").is_ok(),
        "ruby" => which::which("ruby").is_ok(),
        _ => false,
    }
}

// ── small helpers ────────────────────────────────────────────────

fn pretty_lang(lang: &str) -> &str {
    match lang {
        "python" => "Python",
        "nodejs" => "Node.js",
        "php" => "PHP",
        "ruby" => "Ruby",
        other => other,
    }
}

fn home_dir() -> PathBuf {
    let var = if console::is_windows() { "USERPROFILE" } else { "HOME" };
    std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

/// Expand a leading ~ (or ~/…) to the home directory; otherwise take the path
/// as given.
fn expand_tilde(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed == "~" {
        return home_dir();
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }
    PathBuf::from(trimmed)
}

fn prompt(label: &str, default: &str) -> String {
    print!("  {} [{}]: ", label, default.dimmed());
    let _ = io::stdout().flush();
    let mut s = String::new();
    if io::stdin().read_line(&mut s).is_err() {
        return default.to_string();
    }
    let t = s.trim();
    if t.is_empty() {
        default.to_string()
    } else {
        t.to_string()
    }
}

fn run_status(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
