use std::io::{self, Write};
use std::process::Command;

use crate::console::{self, icon_info, icon_ok, icon_play, icon_warn};
use crate::{init, install};
use colored::Colorize;

/// Guided, menu-driven setup. The "getting started" one-liner (install.ps1)
/// ends by calling this. A couple of questions, then it installs the runtime
/// (+ git, + Claude Desktop if chosen) and scaffolds a ready-to-run project.
/// The install steps go through the OS package manager (Chocolatey on Windows,
/// Homebrew on macOS) so the user never types a package command.
pub fn run(dry_run: bool) {
    banner();

    let lang = choose_language();
    let want_ai = choose_ai();
    let name = prompt("Project name", "my-app");

    if dry_run {
        println!();
        println!("  {} Dry run — no changes made. This setup would:", icon_info().blue());
        println!("    - install the {} runtime + package manager (via Chocolatey on Windows, Homebrew on macOS)", lang);
        println!("    - install Git if it's missing");
        if want_ai {
            println!("    - install Claude Desktop + wire the tina4 MCP server");
        }
        println!("    - scaffold the project '{}'", name);
        println!("    - open the \"What's next?\" page with your starting prompt");
        println!();
        return;
    }

    println!();
    println!("{} Setting up — this can take a few minutes...\n", icon_play().green());

    // 1. Language runtime + package manager (Chocolatey/Homebrew/uv under the hood).
    install::run(&lang);

    // 2. Git — needed for version control and future updates.
    ensure_git();

    // 3. Claude Desktop (optional, chosen above).
    if want_ai {
        ensure_claude_desktop();
    }

    // 4. Scaffold the project.
    init::run(Some(&lang), Some(&name));

    // 5. What's next.
    whats_next(&name, want_ai);
}

fn banner() {
    println!();
    println!("  {}", "Tina4 Setup".cyan());
    println!("  {}", "A few questions and you'll be building.".dimmed());
    println!();
}

/// Language menu. Python is the recommended default.
fn choose_language() -> String {
    let opts = [
        ("python", "Python", "recommended — pairs best with AI"),
        ("nodejs", "Node.js", "TypeScript / JavaScript"),
        ("php", "PHP", ""),
        ("ruby", "Ruby", ""),
    ];
    println!("  Which language do you want to build with?");
    for (i, (_, name, desc)) in opts.iter().enumerate() {
        let tag = if i == 0 { "  (default)".green().to_string() } else { String::new() };
        if desc.is_empty() {
            println!("    {}. {}{}", i + 1, name, tag);
        } else {
            println!("    {}. {}{}  {}", i + 1, name, tag, format!("— {}", desc).dimmed());
        }
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

/// How do you want to work? — drives the Claude Desktop install.
fn choose_ai() -> bool {
    println!();
    println!("  How do you want to work?");
    println!("    1. {}  {}", "With Claude Desktop".bold(), "(default) — AI helps you build".dimmed());
    println!("    2. Just my code editor");
    let choice = prompt("Choose 1-2", "1");
    !choice.trim().starts_with('2')
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

fn whats_next(name: &str, want_ai: bool) {
    println!();
    println!("  {} Your Tina4 project '{}' is ready.", icon_ok().green(), name.cyan());
    println!();
    println!("  Next:");
    println!("    cd {}", name);
    println!("    tina4 serve        {}", "# opens http://localhost:7146".dimmed());
    if want_ai {
        println!();
        println!("  Then open {} and paste the prompt from {} to start building.",
            "Claude Desktop".bold(), "START-HERE.md".cyan());
    }
    println!();
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
