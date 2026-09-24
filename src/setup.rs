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
    // Resolved, not named -- see windows_powershell. A spawn that fails here is
    // reported below as "No Administrator rights", which would be the wrong
    // cause. The reporting is deliberately NOT changed: a declined UAC prompt is
    // a normal outcome and has to stay quiet.
    let launched = Command::new(windows_powershell())
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
        let _ = Command::new(windows_powershell())
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

/// Where the AI-skills installer comes from: tina4.com FIRST, then two
/// independent GitHub-backed CDNs, all serving the same bytes for the path.
///
/// One host is not enough, and the FIRST host must not be GitHub. On 2026-09-08
/// a developer's `tina4 update` died on "Error 503 Backend.max_conn reached"
/// from the Varnish tier in front of raw.githubusercontent.com; a later raw
/// incident 503'd every skill file because the jsDelivr `@main` fallback was
/// serving a STALE raw-first installer from cache. The installer these URLs
/// point at is itself tina4.com-first for the skill files, so leading here with
/// the tina4.com bootstrap keeps the whole `tina4 update` walk -- installer AND
/// files -- off GitHub on the common path. jsDelivr and raw stay as fallbacks
/// (retried three times each) for the rare case tina4.com is down.
const SKILLS_INSTALLER_SOURCES_SH: [&str; 3] = [
    "https://tina4.com/install-skills.sh",
    "https://cdn.jsdelivr.net/gh/tina4stack/tina4@main/install-skills.sh",
    "https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh",
];
const SKILLS_INSTALLER_SOURCES_PS1: [&str; 3] = [
    "https://tina4.com/install-skills.ps1",
    "https://cdn.jsdelivr.net/gh/tina4stack/tina4@main/install-skills.ps1",
    "https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.ps1",
];

/// Attempts per source, and the pause between them. The same numbers the
/// installer scripts use for their own downloads.
const SKILLS_FETCH_ATTEMPTS: u32 = 3;
const SKILLS_FETCH_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// A ceiling on the whole walk.
///
/// `curl` is invoked here without a timeout, the same as everywhere else in
/// this client, so a host that accepts a connection and then says nothing can
/// hang for as long as the OS allows. Six attempts where the shipped code made
/// one would multiply that wait by six. Once this much time has gone, no
/// further attempt is started -- a slow update is a bug report of its own, and
/// the point of the retries is a CDN that answers quickly and badly, which is
/// what the reporter hit.
const SKILLS_FETCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// Fetch the installer into `dest`, trying every source in turn.
///
/// A *local* failure does not end this walk, and that is deliberate -- it is
/// the opposite of `download_file`'s rule for release assets. That walk tries
/// different asset names on one host, so a full disk or a dead route dooms
/// every remaining name equally and continuing only buries the real error.
/// These are two different hosts. "Could not reach raw.githubusercontent.com"
/// says nothing at all about jsDelivr, so every source gets its turn and the
/// last failure is what gets reported.
fn fetch_skills_installer(sources: &[&str], dest: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut last = "no source was tried".to_string();
    let mut first = true;
    for url in sources {
        for attempt in 1..=SKILLS_FETCH_ATTEMPTS {
            // The first attempt always runs, however long getting here took.
            if !first && started.elapsed() >= SKILLS_FETCH_BUDGET {
                return Err(format!("{last} (gave up after {}s)", started.elapsed().as_secs()));
            }
            first = false;
            match crate::download_file_classified(url, dest) {
                crate::DownloadOutcome::Ok => return Ok(()),
                crate::DownloadOutcome::Http => {
                    last = format!("{url} answered with an HTTP error")
                }
                crate::DownloadOutcome::Local(cause) => {
                    last = format!("{url}: {}", cause.why())
                }
            }
            // A refused download can still have left a partial or empty file.
            let _ = fs::remove_file(dest);
            if attempt < SKILLS_FETCH_ATTEMPTS {
                std::thread::sleep(SKILLS_FETCH_DELAY);
            }
        }
    }
    Err(last)
}

/// A private directory to stage the installer in.
///
/// `create_dir` fails if the path already exists, so this can never be aimed at
/// something another user planted in a world-writable temp directory -- and the
/// name carries a timestamp as well as the pid so a recycled pid cannot make a
/// stale directory block every future run. On Unix it is then narrowed to 0700,
/// because the next thing that happens to its contents is that a shell runs
/// them.
fn skills_stage_dir() -> io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("tina4-skills-{}-{}", std::process::id(), stamp));
    fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

/// Escape a path for embedding in a single-quoted PowerShell string.
fn ps_single_quote(path: &Path) -> String {
    path.display().to_string().replace('\'', "''")
}

/// The PowerShell one-liner that runs the downloaded skills installer.
///
/// It reads the installer's TEXT and hands it to `iex`, rather than running the
/// file with `-File`. `-File` on a downloaded .ps1 has to clear the execution
/// policy and the mark-of-the-web; a string handed to `iex` clears neither
/// because neither applies to it.
///
/// The text is read with `[System.IO.File]::ReadAllText`, NOT `Get-Content -Raw`.
/// On a real Windows box `Get-Content -Raw` handed `iex` a `[byte[]]` and it
/// refused with "Cannot convert 'System.Byte[]' to the type 'System.String'
/// required by parameter 'Command'". `ReadAllText` always returns a String and
/// auto-detects the BOM, so the (EV-signed) installer runs whatever its encoding.
fn windows_skills_command(target: &str, script: &Path) -> String {
    format!(
        "$ErrorActionPreference='Stop'; $env:TINA4_SKILLS_TARGET='{target}'; iex ([System.IO.File]::ReadAllText('{}'))",
        ps_single_quote(script)
    )
}

/// Which PowerShell to spawn on Windows, as an absolute path.
///
/// `powershell.exe` does not live in `System32`. It lives in
/// `System32\WindowsPowerShell\v1.0\`, and `CreateProcess` given a bare program
/// name searches the application directory, the current directory, `System32`,
/// the Windows directory and then `PATH` -- so the bare name `powershell` can
/// only ever be found through `PATH`. A `PATH` that has lost that one directory
/// fails the spawn outright, with no process and nothing printed.
///
/// The canonical location is tried FIRST, and `PATH` only as a fallback. That
/// ordering is deliberate and is the same shape `download_file_classified` uses
/// for `curl.exe`: the job is to run the real Windows PowerShell, not whatever
/// an earlier `PATH` entry happens to be called. Resolving through `which`
/// first would have been worse than the bug -- `which` accepts any extension in
/// `PATHEXT`, so a `powershell.cmd` in a user-writable directory would win
/// where `CreateProcess`, which only ever appends `.exe`, could not see it at
/// all. `refresh_local_bin_path` prepends exactly such a directory to this
/// process's `PATH` before `ensure_claude_code` spawns.
///
/// Deliberately NOT preferring `pwsh`: choosing a different PowerShell would
/// change which interpreter runs an installer verified against Windows
/// PowerShell 5.1. Finding the same program is the whole job here.
pub(crate) fn windows_powershell() -> String {
    let on_path = which::which("powershell").ok();
    choose_powershell(
        std::env::var("SystemRoot").ok().as_deref(),
        on_path.as_deref(),
        &|candidate| candidate.exists(),
    )
}

/// A path Windows will resolve without consulting anything: `X:\...`.
///
/// Checked explicitly rather than with `Path::is_absolute`, which answers for
/// the platform the code is running on and would call `C:\Windows` relative on
/// Linux -- so the check could never be tested away from Windows.
///
/// UNC (`\\server\share`) is deliberately rejected: `%SystemRoot%` is never a
/// UNC path on a real machine, and spawning an interpreter from one would run
/// code from whatever answers that name.
fn is_windows_drive_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// The decision behind `windows_powershell`, with every lookup passed in so it
/// can be tested away from Windows.
///
/// The two candidates take different types on purpose. They were previously
/// both `Option<String>` and could be swapped at the call site without the
/// compiler or any test noticing, which would have returned `%SystemRoot%`
/// itself -- a directory -- as the program to spawn.
fn choose_powershell(
    system_root: Option<&str>,
    found_on_path: Option<&Path>,
    exists: &dyn Fn(&Path) -> bool,
) -> String {
    // 1. The canonical location, built from %SystemRoot% and confirmed to be
    //    there. An unset, empty, relative or UNC %SystemRoot% falls through
    //    rather than producing a path: an empty one yields
    //    `\System32\...\powershell.exe`, which is drive-RELATIVE and would
    //    resolve against whatever drive the process happens to be on.
    if let Some(root) = system_root {
        // Normalised, not just trimmed: a `C:/Windows` root would otherwise
        // produce a mixed-separator path. Windows accepts it, but the value is
        // also what gets printed back to the user when the spawn fails.
        let root = root.replace('/', "\\");
        let root = root.trim_end_matches('\\');
        if is_windows_drive_absolute(root) {
            let canonical = format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            if exists(Path::new(&canonical)) {
                return canonical;
            }
        }
    }
    // 2. PATH, but only an absolute `.exe` -- the one thing `CreateProcess`
    //    would also have accepted.
    if let Some(found) = found_on_path {
        let text = found.to_string_lossy();
        let is_exe = found
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if is_windows_drive_absolute(&text) && is_exe {
            return text.into_owned();
        }
    }
    // Nothing resolved. Keep the bare name: the spawn fails as it always did,
    // but now it fails with a reason attached.
    "powershell".to_string()
}

fn install_skills_target(target: &str) -> bool {
    println!("  {} Installing tina4 AI skills for {}...", icon_play().green(), target);

    let windows = console::is_windows();
    let sources: &[&str] = if windows {
        &SKILLS_INSTALLER_SOURCES_PS1
    } else {
        &SKILLS_INSTALLER_SOURCES_SH
    };

    let stage = match skills_stage_dir() {
        Ok(dir) => dir,
        Err(e) => {
            println!(
                "  {} Could not create a temporary directory for the skills installer — {}",
                icon_warn().yellow(),
                e
            );
            println!(
                "  {} Skills install skipped — run later: {}",
                icon_warn().yellow(),
                "tina4 ai".cyan()
            );
            return false;
        }
    };
    let script = stage.join(if windows {
        "install-skills.ps1"
    } else {
        "install-skills.sh"
    });

    // Download first, run second.
    //
    // The old shape was `curl -fsSL <url> | sh`, and it could not see a failed
    // download at all: a pipeline exits with the status of its LAST command,
    // and `sh` reading the empty stdin that `curl -f` leaves behind exits 0.
    // So on macOS and Linux a 503 was reported as a successful refresh, with
    // nothing installed and nothing said. Windows was luckier only because
    // PowerShell exits non-zero when `irm` throws -- it printed a raw .NET
    // exception, but it did at least say the refresh had failed.
    if let Err(why) = fetch_skills_installer(sources, &script) {
        let _ = fs::remove_dir_all(&stage);
        println!(
            "  {} Could not download the skills installer — {}",
            icon_warn().yellow(),
            why
        );
        println!(
            "  {} Skills install skipped — run later: {}",
            icon_warn().yellow(),
            "tina4 ai".cyan()
        );
        return false;
    }

    let outcome = if windows {
        // Still `iex` over the installer's text, not `-File` (see
        // windows_skills_command for why). The text is read with
        // [System.IO.File]::ReadAllText, not `Get-Content -Raw`, which handed
        // `iex` a [byte[]] on a real Windows box and was refused.
        //
        // The program is resolved rather than named: see windows_powershell.
        run_status_reason(
            &windows_powershell(),
            &[
                "-NoProfile",
                "-Command",
                &windows_skills_command(target, &script),
            ],
        )
    } else {
        run_status_env_reason("sh", &[&script], &[("TINA4_SKILLS_TARGET", target)])
    };
    let _ = fs::remove_dir_all(&stage);

    if let Err(why) = &outcome {
        // This line is the whole point of RunFailure. Without it a refresh that
        // died at the spawn printed the skip below and nothing else, and the
        // report that reached us had no cause attached to diagnose.
        //
        // No lead of its own: `why` already names the program and says whether
        // it ran. A fixed lead read "The skills installer did not run" over the
        // top of "sh ran and exited 127", which contradicted itself and was
        // wrong about the machine whenever the installer had in fact started.
        println!("  {} {}", icon_warn().yellow(), why);
        println!(
            "  {} Skills install skipped — run later: {}",
            icon_warn().yellow(),
            "tina4 ai".cyan()
        );
    }
    outcome.is_ok()
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
        "ruby" => s.push_str("- **Ruby gotcha:** the handler block is `|request, response|`; pass an HTTP status like `Tina4::HTTP_OK` to `response.json`. The scaffold Gemfile carries `gem \"sqlite3\"` for the default database; `pg`/`mysql2` are add-ons you `bundle add`.\n"),
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

    /// The installer has to be reachable from more than one host. A single
    /// source is what a 503 from raw.githubusercontent.com turned into a
    /// failed `tina4 update`, and collapsing these lists back onto one host
    /// -- or onto two URLs at the same host -- brings that back.
    #[test]
    fn the_skills_installer_has_a_second_host_to_fall_back_to() {
        for sources in [&SKILLS_INSTALLER_SOURCES_SH, &SKILLS_INSTALLER_SOURCES_PS1] {
            let hosts: std::collections::BTreeSet<&str> = sources
                .iter()
                .map(|url| url.split('/').nth(2).expect("every source is an absolute URL"))
                .collect();
            assert!(
                hosts.len() > 1,
                "every installer source resolves to one host: {:?}",
                sources
            );
        }
    }

    /// Both platforms must fetch the same installer, or a fix proven on one
    /// silently misses the other.
    #[test]
    fn both_platforms_draw_on_the_same_set_of_hosts() {
        let hosts = |sources: &[&str]| -> Vec<String> {
            sources
                .iter()
                .map(|url| url.split('/').nth(2).unwrap().to_string())
                .collect()
        };
        assert_eq!(
            hosts(&SKILLS_INSTALLER_SOURCES_SH),
            hosts(&SKILLS_INSTALLER_SOURCES_PS1)
        );
    }

    /// The staged path is interpolated into a single-quoted PowerShell string.
    /// A Windows username with an apostrophe would otherwise end the string and
    /// leave the rest of the path to be parsed as code.
    #[test]
    fn a_quote_in_the_staging_path_cannot_escape_the_powershell_string() {
        assert_eq!(ps_single_quote(Path::new("/tmp/plain")), "/tmp/plain");
        assert_eq!(
            ps_single_quote(Path::new("/tmp/o'brien/install-skills.ps1")),
            "/tmp/o''brien/install-skills.ps1"
        );
    }

    /// A real Windows box refused the installer with "Cannot convert
    /// 'System.Byte[]' to the type 'System.String' required by parameter
    /// 'Command'": `Get-Content -Raw` had handed `iex` a byte array. The
    /// command must read the installer as TEXT via [System.IO.File]::ReadAllText
    /// (always a String), still over `iex` (not `-File`), and never reach for
    /// Get-Content again.
    #[test]
    fn the_windows_installer_is_read_as_text_not_bytes() {
        let cmd = windows_skills_command("codex", Path::new("/tmp/t/install-skills.ps1"));
        assert!(
            cmd.contains("iex ([System.IO.File]::ReadAllText('/tmp/t/install-skills.ps1'))"),
            "must read text and iex it: {cmd}"
        );
        assert!(
            !cmd.contains("Get-Content"),
            "Get-Content -Raw can yield a byte[] that iex refuses: {cmd}"
        );
        assert!(
            !cmd.contains("-File"),
            "-File would reimpose execution policy + mark-of-the-web: {cmd}"
        );
        assert!(
            cmd.contains("$env:TINA4_SKILLS_TARGET='codex'"),
            "target must reach the installer: {cmd}"
        );
    }

    /// The staged path is single-quoted inside the command, so a Windows
    /// username with an apostrophe cannot end the string and run the tail as code.
    #[test]
    fn the_windows_command_single_quotes_the_installer_path() {
        let cmd = windows_skills_command("all", Path::new("/tmp/o'brien/install-skills.ps1"));
        assert!(
            cmd.contains("ReadAllText('/tmp/o''brien/install-skills.ps1')"),
            "apostrophe must be doubled inside the single-quoted arg: {cmd}"
        );
    }

    /// Retrying is worth the wait only while it stays bounded. `curl` is run
    /// without a timeout here, so the ceiling is what keeps six attempts from
    /// costing six hangs.
    #[test]
    fn the_fetch_walk_is_bounded() {
        assert!(SKILLS_FETCH_ATTEMPTS >= 2, "one attempt is what broke");
        let worst_case_sleep = SKILLS_FETCH_DELAY
            * (SKILLS_FETCH_ATTEMPTS - 1)
            * SKILLS_INSTALLER_SOURCES_SH.len() as u32;
        assert!(
            worst_case_sleep < SKILLS_FETCH_BUDGET,
            "the sleeps alone exhaust the budget: {:?} >= {:?}",
            worst_case_sleep,
            SKILLS_FETCH_BUDGET
        );
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

/// Why a child process did not succeed.
///
/// A program that could never be started and a program that ran and failed are
/// different facts about the machine, and a caller that wants to tell the user
/// what went wrong has to be able to say which one happened. The older shape,
/// `.map(|s| s.success()).unwrap_or(false)`, collapsed both into one bare
/// `false` -- which is why a skills refresh that died at the spawn printed its
/// skip line with no cause, on the one platform where nobody could read it.
#[derive(Debug)]
enum RunFailure {
    /// No process was ever created. On Windows this is what a `PATH` that
    /// cannot resolve the program looks like.
    NotStarted { program: String, source: io::Error },
    /// It ran, and came back non-zero.
    Exited { program: String, code: Option<i32> },
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFailure::NotStarted { program, source } => {
                write!(f, "{program} could not be started: {source}")
            }
            RunFailure::Exited {
                program,
                code: Some(code),
            } => write!(f, "{program} ran and exited {code}"),
            RunFailure::Exited { program, code: None } => {
                write!(f, "{program} was terminated before it could exit")
            }
        }
    }
}

/// Run `command`, inheriting both streams, and keep the reason it failed.
fn run_to_completion(program: &str, command: &mut Command) -> Result<(), RunFailure> {
    match command
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(RunFailure::Exited {
            program: program.to_string(),
            code: status.code(),
        }),
        Err(source) => Err(RunFailure::NotStarted {
            program: program.to_string(),
            source,
        }),
    }
}

/// `run_status_reason` with extra environment for the child, and paths passed as
/// arguments rather than interpolated into a shell string -- a temp directory
/// containing a space or a quote is then just a path, not a syntax error.
fn run_status_env_reason(
    cmd: &str,
    args: &[&Path],
    env: &[(&str, &str)],
) -> Result<(), RunFailure> {
    let mut command = Command::new(cmd);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    run_to_completion(cmd, &mut command)
}

fn run_status_reason(cmd: &str, args: &[&str]) -> Result<(), RunFailure> {
    let mut command = Command::new(cmd);
    command.args(args);
    run_to_completion(cmd, &mut command)
}

/// For callers with nothing to say about the failure beyond that there was one.
fn run_status(cmd: &str, args: &[&str]) -> bool {
    run_status_reason(cmd, args).is_ok()
}

#[cfg(test)]
mod spawn_tests {
    use super::{choose_powershell, run_status_reason, RunFailure};
    use std::path::Path;

    const ABSENT: &str = "tina4-no-such-program-b9f2c1";
    const CANONICAL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

    /// A shell that exists on both platforms the tests run on, and a way to
    /// make it fail. Both failure kinds must use the SAME program: comparing a
    /// missing program against a failing one lets a difference in the NAME
    /// satisfy the assertion, and the kinds could then collapse undetected.
    fn failing_shell() -> (&'static str, &'static [&'static str]) {
        if cfg!(windows) {
            ("cmd", &["/C", "exit 3"])
        } else {
            ("sh", &["-c", "exit 3"])
        }
    }

    // ── the failure kinds ───────────────────────────────────────────────

    #[test]
    fn a_program_that_cannot_start_is_reported_as_not_started() {
        let err = run_status_reason(ABSENT, &[]).expect_err("an absent program appeared to succeed");
        match &err {
            RunFailure::NotStarted { program, .. } => assert_eq!(program, ABSENT),
            other => panic!("a missing program was reported as {other:?}"),
        }
        assert!(
            err.to_string().starts_with(ABSENT),
            "the message does not lead with the program that failed: {err}"
        );
    }

    #[test]
    fn a_program_that_runs_and_fails_is_reported_as_exited_and_named() {
        let (cmd, args) = failing_shell();
        let err = run_status_reason(cmd, args).expect_err("`exit 3` appeared to succeed");
        match &err {
            // `program` is asserted, not discarded: without this the field can
            // be filled with anything and the user-facing line degrades to
            // "<unknown> ran and exited 3" with every test still green.
            RunFailure::Exited { program, code } => {
                assert_eq!(*code, Some(3));
                assert_eq!(program, cmd);
            }
            other => panic!("a failing program was reported as {other:?}"),
        }
        assert!(err.to_string().contains(cmd), "{err}");
    }

    /// A child killed before it could exit -- what antivirus or a policy that
    /// terminates the interpreter looks like. `code()` is `None` there, and
    /// that must not read as an ordinary non-zero exit.
    #[test]
    #[cfg(unix)]
    fn a_child_killed_by_a_signal_does_not_read_as_an_exit() {
        let err = run_status_reason("sh", &["-c", "kill -9 $$"]).expect_err("kill -9 looked like success");
        match &err {
            RunFailure::Exited { code, .. } => assert_eq!(*code, None, "a killed child carried an exit code"),
            other => panic!("a killed child was reported as {other:?}"),
        }
        let rendered = err.to_string();
        // Each of these on its own is satisfiable by an empty string, which is
        // how a blanked Display arm survived a mutation run.
        assert!(rendered.contains("sh"), "the message does not name the program: {rendered:?}");
        assert!(rendered.contains("terminated"), "the message does not say it was terminated: {rendered:?}");
        assert!(!rendered.contains("exited"), "a killed child reads as an exit: {rendered}");
    }

    /// The defect itself. Same program on both sides, so only the KIND can
    /// make these differ.
    #[test]
    fn the_two_failure_kinds_do_not_read_alike() {
        let (cmd, args) = failing_shell();
        let started = run_status_reason(cmd, args).unwrap_err().to_string();
        let never = RunFailure::NotStarted {
            program: cmd.to_string(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        }
        .to_string();
        assert_ne!(started, never, "a spawn failure and a non-zero exit still render identically");
        assert!(never.contains("could not be started"), "{never}");
        assert!(started.contains("ran and exited"), "{started}");
    }

    #[test]
    fn a_program_that_succeeds_is_not_a_failure() {
        let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/C", "exit 0"])
        } else {
            ("sh", &["-c", "exit 0"])
        };
        assert!(run_status_reason(cmd, args).is_ok());
    }

    // ── choosing a PowerShell ───────────────────────────────────────────

    /// The canonical location beats PATH. The PATH candidate here is one the
    /// %SystemRoot% branch could never construct, so the two branches cannot be
    /// confused for each other.
    #[test]
    fn the_canonical_location_is_preferred_over_path() {
        let on_path = Path::new(r"D:\tools\ps\powershell.exe");
        assert_eq!(
            choose_powershell(Some(r"C:\Windows"), Some(on_path), &|_| true),
            CANONICAL
        );
    }

    /// ...but only when it is actually there.
    #[test]
    fn path_is_used_when_the_canonical_location_is_absent() {
        let on_path = Path::new(r"D:\tools\ps\powershell.exe");
        assert_eq!(
            choose_powershell(Some(r"C:\Windows"), Some(on_path), &|_| false),
            r"D:\tools\ps\powershell.exe"
        );
    }

    /// The reported failure: PATH has lost the PowerShell directory, so `which`
    /// finds nothing and only the canonical location can save the spawn.
    #[test]
    fn nothing_on_path_falls_back_to_the_canonical_location() {
        assert_eq!(choose_powershell(Some(r"C:\Windows"), None, &|_| true), CANONICAL);
    }

    #[test]
    fn a_non_default_system_root_is_honoured_rather_than_hardcoded() {
        let chosen = choose_powershell(Some(r"D:\OtherWindows"), None, &|_| true);
        assert!(chosen.starts_with(r"D:\OtherWindows\"), "the fallback ignored %SystemRoot%: {chosen}");
    }

    #[test]
    fn a_trailing_separator_on_system_root_does_not_double_up() {
        assert_eq!(choose_powershell(Some(r"C:\Windows\"), None, &|_| true), CANONICAL);
        assert_eq!(choose_powershell(Some("C:/Windows/"), None, &|_| true), CANONICAL);
    }

    /// An empty %SystemRoot% would build `\System32\...\powershell.exe`, which
    /// is drive-RELATIVE: Windows resolves it against whatever drive the
    /// process is on, which may be removable or a mapped share.
    #[test]
    fn an_empty_system_root_never_becomes_a_drive_relative_path() {
        let chosen = choose_powershell(Some(""), None, &|_| true);
        assert_eq!(chosen, "powershell");
        assert!(!chosen.starts_with('\\'), "built a drive-relative program path: {chosen}");
    }

    #[test]
    fn a_relative_or_unc_system_root_is_refused() {
        for root in ["Windows", r"..\Windows", r"\\server\share\Windows", r"\Windows"] {
            assert_eq!(
                choose_powershell(Some(root), None, &|_| true),
                "powershell",
                "a {root} SystemRoot was turned into a program path"
            );
        }
    }

    /// `which` accepts every extension in PATHEXT; `CreateProcess` only ever
    /// appends `.exe`. Honouring a `.cmd` would run a different program than
    /// the one the bare name could ever have reached -- and
    /// `refresh_local_bin_path` puts a user-writable directory on PATH first.
    #[test]
    fn a_path_result_that_is_not_an_exe_is_refused() {
        for bad in [r"C:\Users\x\.local\bin\powershell.cmd", r"C:\x\powershell.bat", r"C:\x\powershell"] {
            assert_eq!(
                choose_powershell(None, Some(Path::new(bad)), &|_| false),
                "powershell",
                "{bad} was accepted as the program to spawn"
            );
        }
    }

    /// An empty PATH entry makes `which` answer with a cwd-relative name.
    #[test]
    fn a_path_result_that_is_not_absolute_is_refused() {
        for bad in ["powershell.exe", r".\powershell.exe", r"sub\powershell.exe"] {
            assert_eq!(choose_powershell(None, Some(Path::new(bad)), &|_| false), "powershell", "{bad} was accepted");
        }
    }

    #[test]
    fn with_nothing_to_go_on_the_bare_name_survives() {
        assert_eq!(choose_powershell(None, None, &|_| true), "powershell");
    }
}
