use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::console::{self, icon_info, icon_ok, icon_play, icon_warn};
use crate::{init, install};
use colored::Colorize;

/// Which AI tool the developer wants to build with.
#[derive(Clone, Copy, PartialEq)]
enum AiChoice {
    ClaudeDesktop,
    ClaudeCode,
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
pub fn run(dry_run: bool, skip_install: bool) {
    banner();

    // First run configures the machine (language, AI tool, projects folder) and
    // remembers those choices. Every run after that is a fast path: it only asks
    // the project type + name and builds into the environment already set up.
    match load_config() {
        Some(cfg) => run_quick(dry_run, skip_install, cfg),
        None => run_first(dry_run, skip_install),
    }
}

/// First-time setup: ask everything, install the world, scaffold the first
/// project, and remember the projects folder + AI tool for next time.
fn run_first(dry_run: bool, skip_install: bool) {
    // If this is the elevated re-run, the answers were handed to us through the
    // environment — the menu already ran in the user's own console, so skip
    // straight to installing + scaffolding (no second round of questions).
    if let Some((lang, ai, projects_dir, name)) = elevated_answers() {
        let project_path = projects_dir.join(&name);
        run_first_install(&lang, ai, &projects_dir, &project_path, skip_install);
        return;
    }

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

    // A real install goes through Chocolatey, which needs Administrator rights.
    // Now that we have the answers, hand off to an elevated window that runs
    // the install + scaffold without re-asking. No-op when already elevated /
    // already admin / off Windows — we just continue in-process below.
    if !skip_install {
        elevate_for_install(&lang, ai, &projects_dir, &name);
    }

    run_first_install(&lang, ai, &projects_dir, &project_path, skip_install);
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
) {
    println!();
    println!("{} Setting up — this can take a few minutes...\n", icon_play().green());

    if skip_install {
        println!(
            "  {} --skip-install: skipping runtime / git / AI / skills installs\n",
            icon_info().blue()
        );
    } else {
        // 1. Language runtime + package manager (Chocolatey/Homebrew/uv under the hood).
        install::run(lang);
        // 2. Git — version control + future updates.
        ensure_git();
        // 3. The tina4 AI skills, installed globally so whatever AI tool the
        //    user picks already knows how to build with tina4.
        install_skills_global();
        // 4. The chosen AI tool.
        match ai {
            AiChoice::ClaudeDesktop => ensure_claude_desktop(),
            AiChoice::ClaudeCode => ensure_claude_code(),
            AiChoice::None => {}
        }
        // Remember the environment so future `tina4 setup` runs are one-question
        // quick. Only after a real install — a --skip-install test machine isn't
        // actually configured.
        save_config(projects_dir, ai);
    }

    scaffold_into(projects_dir, project_path, lang, ai);
    pause_if_elevated();
}

/// Every run after the first: the machine is already set up, so only ask the
/// project type + name, then build into the saved projects folder.
fn run_quick(dry_run: bool, skip_install: bool, cfg: SetupConfig) {
    // Elevated re-run: reuse the answers passed through the environment instead
    // of asking again in the new window.
    let (lang, name) = if let Some((lang, _ai, _dir, name)) = elevated_answers() {
        (lang, name)
    } else {
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
        (lang, name)
    };
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
    // machine doesn't have yet. Pass the answers so the elevated window runs
    // straight through without re-asking.
    if need_runtime && !skip_install {
        elevate_for_install(&lang, cfg.ai, &cfg.projects_dir, &name);
    }

    println!();
    println!("{} Building your project...\n", icon_play().green());

    if skip_install {
        println!("  {} --skip-install: skipping runtime install\n", icon_info().blue());
    } else if need_runtime {
        install::run(&lang);
    } else {
        println!("  {} {} runtime already installed", icon_ok().green(), pretty_lang(&lang));
    }

    scaffold_into(&cfg.projects_dir, &project_path, &lang, cfg.ai);
    pause_if_elevated();
}

/// Shared tail for both modes: create the projects folder, scaffold the
/// project inside it, write its CLAUDE.md, open the tool, print next steps.
fn scaffold_into(projects_dir: &Path, project_path: &Path, lang: &str, ai: AiChoice) {
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

    write_project_claude_md(project_path, lang, ai);
    open_ide(ai);
    whats_next(project_path, ai);
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
/// We currently know Claude Desktop (chat app) and Claude Code (terminal).
fn choose_ai() -> AiChoice {
    println!();
    println!("  Which AI tool do you want to build with?");
    println!("    1. {}  {}", "Claude Desktop".bold(), "(default) — chat app, best for non-coders".dimmed());
    println!("    2. {}  {}", "Claude Code".bold(), "— AI in your terminal".dimmed());
    println!("    3. {}  {}", "Just my code editor".bold(), "— no AI".dimmed());
    let choice = prompt("Choose 1-3", "1");
    match choice.trim() {
        "2" => AiChoice::ClaudeCode,
        "3" => AiChoice::None,
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
        } else {
            println!("    - install Homebrew if missing");
        }
        println!("    - install the {} runtime + tools through it", lang);
        println!("    - install Git if missing");
        println!("    - install the tina4 AI skills globally (~/.claude/skills)");
        match ai {
            AiChoice::ClaudeDesktop => println!("    - install Claude Desktop"),
            AiChoice::ClaudeCode => println!("    - install Claude Code"),
            AiChoice::None => {}
        }
    }
    println!("    - create your projects folder: {}", projects_dir.display());
    println!("    - scaffold '{}' at {}", name, project_path.display());
    println!("    - write a CLAUDE.md into the project with instructions");
    match ai {
        AiChoice::ClaudeDesktop => println!("    - open Claude Desktop"),
        AiChoice::ClaudeCode => println!("    - show how to start Claude Code in the project"),
        AiChoice::None => {}
    }
    println!();
}

/// `choco install` needs Administrator rights. If we're not elevated, relaunch
/// `tina4 setup` through UAC and hand off to that elevated instance — passing
/// the already-collected answers via the environment so the elevated window
/// runs straight through without asking the questions again. No-op off Windows,
/// when already elevated, or when already admin (caller continues in-process).
///
/// Called AFTER the menu so the questions always run in the user's own console;
/// elevating first would relaunch into a new window and leave the original
/// looking like it just exited.
fn elevate_for_install(lang: &str, ai: AiChoice, projects_dir: &Path, name: &str) {
    if !console::is_windows() || std::env::var("TINA4_SETUP_ELEVATED").is_ok() {
        return;
    }
    if is_admin_windows() {
        return;
    }

    println!();
    println!("  {} Setup needs Administrator rights to install software.", icon_info().blue());
    println!("  {} Approve the Windows prompt — a new window will finish the install.", icon_info().blue());
    println!();

    let Ok(exe) = std::env::current_exe() else { return };
    // Start-Process … -Verb RunAs raises the UAC prompt; the elevated child
    // re-runs `setup` with the answers in its environment (so it skips the
    // menu). TINA4_SETUP_ELEVATED guards against re-elevating.
    let q = |s: &str| s.replace('\'', "''");
    let cmd = format!(
        "$env:TINA4_SETUP_ELEVATED='1'; \
         $env:TINA4_SETUP_LANG='{lang}'; \
         $env:TINA4_SETUP_AI='{ai}'; \
         $env:TINA4_SETUP_DIR='{dir}'; \
         $env:TINA4_SETUP_NAME='{name}'; \
         Start-Process -FilePath '{exe}' -ArgumentList 'setup' -Verb RunAs",
        lang = q(lang),
        ai = ai_env(ai),
        dir = q(&projects_dir.display().to_string()),
        name = q(name),
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
    println!(
        "  {} Couldn't elevate automatically. Right-click your terminal, choose \
         'Run as administrator', then run: tina4 setup",
        icon_warn().yellow()
    );
    std::process::exit(1);
}

/// The elevated re-run receives the menu answers through the environment so it
/// doesn't ask them again in its new window. Returns them only when we ARE that
/// elevated re-run (TINA4_SETUP_ELEVATED set + the answers present).
fn elevated_answers() -> Option<(String, AiChoice, PathBuf, String)> {
    if std::env::var("TINA4_SETUP_ELEVATED").is_err() {
        return None;
    }
    let lang = std::env::var("TINA4_SETUP_LANG").ok()?;
    let ai = match std::env::var("TINA4_SETUP_AI").ok()?.as_str() {
        "code" => AiChoice::ClaudeCode,
        "none" => AiChoice::None,
        _ => AiChoice::ClaudeDesktop,
    };
    let dir = PathBuf::from(std::env::var("TINA4_SETUP_DIR").ok()?);
    let name = std::env::var("TINA4_SETUP_NAME").ok()?;
    Some((lang, ai, dir, name))
}

/// Stable env token for an AI choice (round-trips through `elevated_answers`).
fn ai_env(ai: AiChoice) -> &'static str {
    match ai {
        AiChoice::ClaudeDesktop => "desktop",
        AiChoice::ClaudeCode => "code",
        AiChoice::None => "none",
    }
}

/// In the elevated re-run the new window closes the instant setup returns —
/// hold it open so the user can read the result.
fn pause_if_elevated() {
    if std::env::var("TINA4_SETUP_ELEVATED").is_ok() {
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

fn ensure_claude_code() {
    if which::which("claude").is_ok() {
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
fn install_skills_global() {
    println!("  {} Installing tina4 AI skills (global)...", icon_play().green());
    let ok = if console::is_windows() {
        run_status(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "irm https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.ps1 | iex",
            ],
        )
    } else {
        run_status(
            "sh",
            &["-c", "curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | sh"],
        )
    };
    if !ok {
        println!(
            "  {} Skills install skipped — run later: {}",
            icon_warn().yellow(),
            "tina4 ai".cyan()
        );
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
    let ai_line = match ai {
        AiChoice::ClaudeDesktop => "You are working through **Claude Desktop**.",
        AiChoice::ClaudeCode => "You are working through **Claude Code** (terminal).",
        AiChoice::None => "This project is set up for AI-assisted development.",
    };
    let content = format!(
        r#"# {name} — Tina4 {lang} project

{ai_line}

This is a **Tina4 v3** project — *The Intelligent Native Application 4ramework*.
Tina4 is **built for AI**: zero third-party dependencies, convention over
configuration, and a small, consistent API that fits in context. Prefer the
framework's built-ins (ORM, routing, queues, auth, templates, WebSockets, …)
over any library, and don't guess API names — they're documented exactly (see
**Help & source of truth** below).

- Website & docs: **https://tina4.com** — the **"Ask Tina4"** box there is
  RAG-backed; ask it any framework question and it answers from the live corpus.
- This running app has a dev dashboard at **`/__dev`** (route inspector, DB
  runner, logs, AI chat) and a live MCP endpoint at **`/__dev/mcp`** an AI can
  query for THIS project's real routes, models, and API signatures.

## Help & source of truth (check before guessing)

1. **Installed skills** in `~/.claude/skills/` — **tina4-developer** and
   **tina4-js**. The authoritative guides for the framework + reactive frontend;
   use them, they document the real API surface.
2. **https://tina4.com** — full docs + **Ask Tina4** (RAG search over the live
   framework corpus).
3. **`/__dev/mcp`** on the running app — live API reflection (`api_search`,
   docs + route/model listing) for this exact project and framework version.

## How to run

```bash
tina4 serve
```

Starts the dev server, watches your files, hot-reloads the browser, and opens
the app **and** the `/__dev` dashboard in a second tab. The URL is printed when
it starts.

> Dev runs two ports: the **base** port hot-reloads (for you), and **base+1000**
> is a stable port that does NOT reload — use that one when an AI is driving the
> browser so a reload doesn't interrupt it.

## Where things go

| You want to…            | Put it in…                          | Make it with                         |
|-------------------------|-------------------------------------|--------------------------------------|
| Add a page or API route | `src/routes/`                       | `tina4 generate route <name>`        |
| Add a database model    | `src/orm/`                          | `tina4 generate model <Name>`        |
| Change the schema       | `migrations/`                       | `tina4 generate migration <name>` then `tina4 migrate` |
| Add a page template     | `src/templates/`                    | (Twig-style templates)               |
| Frontend behaviour      | served at `/js/tina4js.min.js`      | tina4-js signals + html templates    |

## Golden rules

- **Use Tina4 v3 built-ins first** — it's zero-dependency; reach for the
  framework (and the skills) before any library or hand-rolled code.
- Don't guess API names — check the skills, **Ask Tina4** at https://tina4.com,
  or the live `/__dev/mcp` tools.
- Routes return data; `response()` auto-serializes models and lists to JSON.
- Env vars are read from `.env` (already created). `TINA4_DEBUG=true` is on for dev.
- All links and references should point to **https://tina4.com**.

## A good first prompt

> "Add a `/products` page backed by a `Product` model (name, price, image_url),
> seed three rows, and render them as cards using a tina4-js component."
"#,
        name = project_path.file_name().and_then(|s| s.to_str()).unwrap_or("app"),
        lang = pretty_lang(lang),
        ai_line = ai_line,
    );
    match fs::write(&file, content) {
        Ok(_) => println!("  {} Wrote {}", icon_ok().green(), "CLAUDE.md".cyan()),
        Err(e) => eprintln!("  {} Could not write CLAUDE.md: {}", icon_warn().yellow(), e),
    }
}

/// Best-effort: bring the chosen AI tool to the foreground. Never fatal —
/// whats_next() always prints the manual command too.
fn open_ide(ai: AiChoice) {
    if ai != AiChoice::ClaudeDesktop {
        return;
    }
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").args(["-a", "Claude"]).status()
    } else if console::is_windows() {
        Command::new("cmd").args(["/C", "start", "", "claude"]).status()
    } else {
        return;
    };
}

fn whats_next(project_path: &Path, ai: AiChoice) {
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
    match ai {
        AiChoice::ClaudeDesktop => {
            println!(
                "  Then open {} and paste the first-prompt idea from {}.",
                "Claude Desktop".bold(),
                "CLAUDE.md".cyan()
            );
        }
        AiChoice::ClaudeCode => {
            println!("  Then start AI in your project:");
            println!("    cd {} && claude", p);
        }
        AiChoice::None => {
            println!("  Open the project in your editor and read {}.", "CLAUDE.md".cyan());
        }
    }
    println!();

    // Offer to launch it right now — cd into the project and `tina4 serve`,
    // which opens the browser on the running app. Skipped in the elevated
    // Windows install window (the user serves from their own console there;
    // the printed command above is the fallback).
    if std::env::var("TINA4_SETUP_ELEVATED").is_ok() {
        return;
    }
    let ans = prompt("Start it now and open it in your browser? [Y/n]", "y");
    if matches!(ans.trim().to_lowercase().as_str(), "" | "y" | "yes") {
        let label = project_path.file_name().and_then(|s| s.to_str()).unwrap_or("your app");
        println!();
        println!(
            "  {} Starting {} — your browser will open. Press Ctrl+C to stop.",
            icon_play().green(),
            label.cyan()
        );
        println!();
        // Re-exec ourselves as `tina4 serve` inside the project so it picks up
        // app.py/index.php/app.rb/app.ts and serves THIS project.
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tina4"));
        let _ = Command::new(exe).arg("serve").current_dir(project_path).status();
    } else {
        println!("  {} No problem — run {} when you're ready.", icon_info().blue(), "tina4 serve".cyan());
    }
}

// ── config (zero-dep key=value file) ─────────────────────────────

fn config_path() -> PathBuf {
    home_dir().join(".tina4").join("setup.conf")
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
        AiChoice::None => "none",
    }
}

fn ai_from_str(s: &str) -> AiChoice {
    match s {
        "claude-desktop" => AiChoice::ClaudeDesktop,
        "claude-code" => AiChoice::ClaudeCode,
        _ => AiChoice::None,
    }
}

fn ai_label(ai: AiChoice) -> &'static str {
    match ai {
        AiChoice::ClaudeDesktop => "Claude Desktop",
        AiChoice::ClaudeCode => "Claude Code",
        AiChoice::None => "code editor only",
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
