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

    // A real install goes through Chocolatey, which needs Administrator rights.
    // Preview / scaffold-only modes change no system state, so they never
    // trigger the UAC prompt.
    if !dry_run && !skip_install {
        ensure_admin_windows();
    }

    let lang = choose_language();
    let ai = choose_ai();
    let projects_dir = choose_projects_dir();
    let name = prompt("Name of your first project", "tina4example");
    let project_path = projects_dir.join(&name);

    if dry_run {
        print_plan(&lang, ai, &projects_dir, &name, &project_path, skip_install);
        return;
    }

    println!();
    println!("{} Setting up — this can take a few minutes...\n", icon_play().green());

    if skip_install {
        println!(
            "  {} --skip-install: skipping runtime / git / AI / skills installs\n",
            icon_info().blue()
        );
    } else {
        // 1. Language runtime + package manager (Chocolatey/Homebrew/uv under the hood).
        install::run(&lang);
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
    }

    // 5. Projects folder + scaffold the first project inside it.
    if let Err(e) = fs::create_dir_all(&projects_dir) {
        eprintln!("  {} Could not create {}: {}", icon_warn().yellow(), projects_dir.display(), e);
    } else {
        println!("  {} Projects folder: {}", icon_ok().green(), projects_dir.display().to_string().cyan());
    }
    if let Err(e) = std::env::set_current_dir(&projects_dir) {
        eprintln!("  {} Could not enter {}: {}", icon_warn().yellow(), projects_dir.display(), e);
    }
    // Setup owns the ending (CLAUDE.md, open IDE, next steps), so tell init not
    // to grab the terminal with its blocking "start server now?" prompt.
    std::env::set_var("TINA4_INIT_NO_SERVE", "1");
    init::run(Some(&lang), Some(&name));

    // 6. A project CLAUDE.md so the AI tool knows how to work in this project.
    write_project_claude_md(&project_path, &lang, ai);

    // 7. Open the chosen tool (best effort — never fatal).
    open_ide(ai);

    // 8. What's next.
    whats_next(&project_path, ai);
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
/// `tina4 setup` through UAC and hand off to that elevated instance. No-op off
/// Windows and when we're already elevated, so it never loops.
fn ensure_admin_windows() {
    if !console::is_windows() || std::env::var("TINA4_SETUP_ELEVATED").is_ok() {
        return;
    }
    if is_admin_windows() {
        return;
    }

    println!("  {} Setup needs Administrator rights to install software.", icon_info().blue());
    println!("  {} Approve the Windows prompt — setup continues in a new window.", icon_info().blue());
    println!();

    let Ok(exe) = std::env::current_exe() else { return };
    // Start-Process … -Verb RunAs raises the UAC prompt; the elevated child
    // re-runs `setup`. TINA4_SETUP_ELEVATED guards against re-elevating.
    let exe_str = exe.display().to_string().replace('\'', "''");
    let cmd = format!(
        "$env:TINA4_SETUP_ELEVATED='1'; \
         Start-Process -FilePath '{exe_str}' -ArgumentList 'setup' -Verb RunAs"
    );
    let launched = Command::new("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if launched {
        std::process::exit(0);
    }
    println!(
        "  {} Couldn't elevate automatically. Right-click your terminal, choose \
         'Run as administrator', then run: tina4 setup",
        icon_warn().yellow()
    );
    std::process::exit(1);
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
    // Next pass: write the tina4 MCP server into claude_desktop_config.json so
    // Claude can drive this project (24 dev tools). Tracked in the setup spec.
}

fn ensure_claude_code() {
    if which::which("claude").is_ok() {
        println!("  {} Claude Code already installed", icon_ok().green());
        return;
    }
    println!("  {} Installing Claude Code...", icon_play().green());
    // npm is the reliable cross-platform path when Node is present (it is, if
    // the user picked Node; otherwise it may not be).
    let installed = which::which("npm").is_ok()
        && run_status("npm", &["install", "-g", "@anthropic-ai/claude-code"]);
    if installed || which::which("claude").is_ok() {
        println!("  {} Claude Code installed", icon_ok().green());
    } else {
        println!(
            "  {} Install Claude Code from {} (needs Node.js, or use the native installer there)",
            icon_info().blue(),
            "https://docs.claude.com/claude-code".cyan()
        );
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

This is a **Tina4** project: a self-contained backend that also serves a
[tina4-js](https://github.com/tina4stack/tina4-js) reactive frontend. Zero
external dependencies, batteries included.

## How to run

```bash
tina4 serve
```

This starts the dev server, watches your files, and hot-reloads the browser on
every change. The URL is printed when it starts.

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

## Skills

The **tina4-developer** and **tina4-js** skills are installed globally. Use
them — they are the source of truth for tina4 patterns. Don't guess API names;
the framework is small and consistent, and the skills document it exactly.

## Golden rules

- Keep it simple. Tina4 is zero-dependency — reach for the framework before a library.
- Routes return data; `response()` auto-serializes models and lists to JSON.
- Env vars are read from `.env` (already created). `TINA4_DEBUG=true` is on for dev.

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
    println!("  Start it:");
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
