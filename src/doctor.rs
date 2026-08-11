use crate::console::{icon_ok, icon_fail, icon_play, icon_info, icon_warn};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The user-facing installer for the global Tina4 AI skills. `tina4 doctor` reads
/// its pinned `ref` to learn the latest skills version, and prints it as the
/// refresh command. Fetch-source and refresh-command are the SAME URL on purpose,
/// so what doctor reports as "latest" is exactly what a refresh would install.
const SKILLS_INSTALL_URL: &str = "https://tina4.com/install-skills.sh";
const SKILLS_INSTALL_CMD: &str = "tina4 skills claude";
const CODEX_SKILLS_INSTALL_CMD: &str = "tina4 skills codex";

struct ToolCheck {
    name: &'static str,
    commands: &'static [&'static str],
    version_flag: &'static str,
    pkg_manager: Option<PkgCheck>,
}

struct PkgCheck {
    name: &'static str,
    commands: &'static [&'static str],
    version_flag: &'static str,
}

struct CliCheck {
    name: &'static str,
    lang: &'static str,
    install_cmd: &'static str,
}

pub fn run() {
    println!(
        "\n{}",
        "  Tina4 Doctor — Environment Check  ".on_bright_black().white()
    );
    println!();

    let tools = [
        ToolCheck {
            name: "Python",
            commands: &["python3", "python"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "uv",
                commands: &["uv"],
                version_flag: "--version",
            }),
        },
        ToolCheck {
            name: "PHP",
            commands: &["php"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "composer",
                commands: &["composer", "composer.bat"],
                version_flag: "--version",
            }),
        },
        ToolCheck {
            name: "Ruby",
            commands: &["ruby"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "bundler",
                commands: &["bundle", "bundle.bat"],
                version_flag: "--version",
            }),
        },
        ToolCheck {
            name: "Node.js",
            commands: &["node"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "npm",
                commands: &["npm", "npm.cmd"],
                version_flag: "--version",
            }),
        },
    ];

    println!(
        "  {:<12} {:<10} {:<20} {:<12} {}",
        "Language".bold(),
        "Status".bold(),
        "Version".bold(),
        "Pkg Mgr".bold(),
        "Version".bold()
    );
    println!("  {}", "─".repeat(70));

    for tool in &tools {
        let (status, version) = check_tool(tool.commands, tool.version_flag);
        let (pkg_status, pkg_name, pkg_version) = if let Some(ref pkg) = tool.pkg_manager {
            let (s, v) = check_tool(pkg.commands, pkg.version_flag);
            (s, pkg.name, v)
        } else {
            (false, "—", "—".into())
        };

        let status_icon = if status {
            icon_ok().green().to_string()
        } else {
            icon_fail().red().to_string()
        };
        let pkg_icon = if pkg_status {
            icon_ok().green().to_string()
        } else if tool.pkg_manager.is_some() {
            icon_fail().red().to_string()
        } else {
            "-".dimmed().to_string()
        };

        let ver = if status {
            version.cyan().to_string()
        } else {
            "not installed".dimmed().to_string()
        };
        let pkg_ver = if pkg_status {
            pkg_version.cyan().to_string()
        } else if tool.pkg_manager.is_some() {
            "not installed".dimmed().to_string()
        } else {
            "—".dimmed().to_string()
        };

        println!(
            "  {:<12} {:<10} {:<20} {} {:<10} {}",
            tool.name, status_icon, ver, pkg_icon, pkg_name, pkg_ver
        );
    }

    // --- Tina4 CLIs ---
    println!();
    println!("  {}", "Tina4 CLIs".bold());
    println!("  {}", "─".repeat(70));

    let clis = [
        CliCheck { name: "tina4python", lang: "Python",  install_cmd: "pip install tina4-python" },
        CliCheck { name: "tina4php",    lang: "PHP",     install_cmd: "composer global require tina4stack/tina4php" },
        CliCheck { name: "tina4ruby",   lang: "Ruby",    install_cmd: "gem install tina4ruby" },
        CliCheck { name: "tina4nodejs", lang: "Node.js", install_cmd: "npm install -g tina4-nodejs" },
        CliCheck { name: "vite",       lang: "tina4js",  install_cmd: "npm install vite" },
    ];

    for cli in &clis {
        // Check global PATH first, then project-local paths
        let local_paths: &[&str] = match cli.name {
            "tina4php" => &["vendor/bin/tina4php", "bin/tina4php"],
            "tina4python" => &[".venv/bin/tina4python", "venv/bin/tina4python"],
            "tina4ruby" => &["bin/tina4ruby", "exe/tina4ruby"],
            "tina4nodejs" => &["node_modules/.bin/tina4nodejs", "packages/cli/src/bin.ts"],
            "vite" => &["node_modules/.bin/vite"],
            _ => &[],
        };
        let found_global = which::which(cli.name).is_ok();
        let found_local = local_paths.iter().any(|p| Path::new(p).exists());
        let found = found_global || found_local;
        let icon = if found {
            icon_ok().green().to_string()
        } else {
            icon_fail().red().to_string()
        };
        let status_text = if found_global {
            "installed (global)".cyan().to_string()
        } else if found_local {
            "installed (project)".cyan().to_string()
        } else {
            format!(
                "{}  {}  {}",
                "not found".dimmed(),
                "→".dimmed(),
                format!("run: {}", cli.install_cmd).yellow()
            )
        };
        println!("  {} {:<16} {:<12} {}", icon, cli.name, cli.lang, status_text);
    }

    // --- Tina4 AI skills (global ~/.claude/skills) ---
    check_skills_currency();
    check_codex_skills_currency();

    // --- macOS build tools (Xcode Command Line Tools) ---
    // Homebrew, git, and the PHP/Ruby/Node runtimes need these; Python (uv) does
    // not. On a fresh Mac they're the usual reason `tina4 setup` can't install.
    if cfg!(target_os = "macos") {
        println!();
        println!("  {}", "Build tools (macOS)".bold());
        println!("  {}", "─".repeat(70));
        let clt = std::process::Command::new("xcode-select")
            .arg("-p")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if clt {
            println!(
                "  {} {:<28} {}",
                icon_ok().green(),
                "Xcode Command Line Tools",
                "installed".cyan()
            );
        } else {
            println!(
                "  {} {:<28} {}  {}  {}",
                icon_fail().red(),
                "Xcode Command Line Tools",
                "not installed".dimmed(),
                "→".dimmed(),
                "needed for Homebrew/git/PHP/Ruby/Node — run: xcode-select --install (Python needs none)".yellow()
            );
        }
    }

    // --- Port availability ---
    println!();
    println!("  {}", "Ports".bold());
    println!("  {}", "─".repeat(70));

    let ports = [
        (7145u16, "PHP"),
        (7146u16, "Python"),
        (7147u16, "Ruby"),
        (7148u16, "Node.js"),
        (5173u16, "tina4js"),
    ];

    for (port, lang) in &ports {
        let free = std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok();
        let icon = if free {
            icon_ok().green().to_string()
        } else {
            icon_warn().yellow().to_string()
        };
        let status = if free {
            "free".green().to_string()
        } else {
            format!("{}", "in use".yellow())
        };
        println!(
            "  {} {:<6} ({:<8}) {}",
            icon, port, lang, status
        );
    }

    // --- Windows PATH sanity check ---
    #[cfg(windows)]
    {
        println!();
        println!("  {}", "Windows PATH".bold());
        println!("  {}", "─".repeat(70));
        check_windows_path();
    }

    // --- Current project detection ---
    println!();
    match crate::detect::detect_language() {
        Some(info) => {
            println!(
                "  {} Current directory: {} project",
                icon_play().green(),
                info.language.cyan()
            );
        }
        None => {
            println!(
                "  {} No Tina4 project detected in current directory",
                icon_info().blue()
            );
        }
    }

    println!();
}

#[cfg(windows)]
fn check_windows_path() {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let path_lower = path_var.to_lowercase();

    let checks = [
        ("pip/Python Scripts", &["scripts", "python\\scripts", "python3\\scripts"][..]),
        ("npm global",         &["npm\\node_modules\\.bin", "roaming\\npm"][..]),
        ("gem executables",    &["ruby\\bin", "gems\\bin"][..]),
        ("composer global",    &["composer\\vendor\\bin"][..]),
    ];

    for (label, needles) in &checks {
        let found = needles.iter().any(|n| path_lower.contains(n));
        let icon = if found {
            icon_ok().green().to_string()
        } else {
            icon_warn().yellow().to_string()
        };
        let status = if found {
            "in PATH".green().to_string()
        } else {
            "may not be in PATH".yellow().to_string()
        };
        println!("  {} {:<25} {}", icon, label, status);
    }
}

fn check_tool(commands: &[&str], version_flag: &str) -> (bool, String) {
    for cmd in commands {
        if let Ok(output) = Command::new(crate::console::resolve_cmd(cmd))
            .arg(version_flag)
            .output()
        {
            if output.status.success() {
                let raw = String::from_utf8_lossy(&output.stdout).to_string();
                let version = extract_version_number(&raw);
                return (true, version);
            }
        }
    }
    (false, String::new())
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn extract_version_number(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let first_line = clean.lines().next().unwrap_or("");
    for word in first_line.split_whitespace() {
        let trimmed = word.trim_start_matches('v');
        if trimmed.contains('.') && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return trimmed.to_string();
        }
    }
    first_line.trim().to_string()
}

// ─── Tina4 AI skills currency ────────────────────────────────────────────────
//
// `tina4 doctor` reports whether the globally-installed skills in
// ~/.claude/skills are current with the latest published release. This whole
// path is STRICTLY READ-ONLY: it reads a global marker + fetches the installer's
// pinned ref, and prints. It writes nothing, and it NEVER touches a project's
// CLAUDE.md (the only writer, install-skills, only touches ~/.claude/skills).

#[derive(Debug, PartialEq, Eq)]
enum SkillsStatus {
    NotInstalled,
    Current(String),
    Stale { installed: String, latest: String },
    InstalledUnknownLatest(String), // marker known, latest unknown (offline)
    UnknownInstalled(Option<String>), // no marker; latest maybe known
}

/// Pure decision: given whether the skills dir exists, the recorded installed
/// ref, and the latest published ref, classify currency. No IO - unit-tested.
fn classify_skills(dir_exists: bool, installed: Option<String>, latest: Option<String>) -> SkillsStatus {
    if !dir_exists {
        return SkillsStatus::NotInstalled;
    }
    match (installed, latest) {
        (Some(i), Some(l)) if i == l => SkillsStatus::Current(i),
        (Some(i), Some(l)) => SkillsStatus::Stale { installed: i, latest: l },
        (Some(i), None) => SkillsStatus::InstalledUnknownLatest(i),
        (None, l) => SkillsStatus::UnknownInstalled(l),
    }
}

/// Pull the pinned version out of the installer script's
/// `ref="${TINA4_SKILLS_REF:-X.Y.Z}"` default. Pure - unit-tested.
fn parse_ref_from_installer(script: &str) -> Option<String> {
    let marker = "TINA4_SKILLS_REF:-";
    let start = script.find(marker)? + marker.len();
    let rest = &script[start..];
    let end = rest.find(|c: char| c == '}' || c == '"' || c == '\'' || c.is_whitespace())?;
    let v = rest[..end].trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the ref recorded by install-skills (global marker). None if never
/// installed by a marker-aware installer.
fn read_installed_skills_ref(skills_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(skills_dir.join(".tina4-skills-ref")).ok()?;
    let v = content.trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

fn has_legacy_developer_skill(skills_dir: &Path) -> bool {
    skills_dir.join("tina4-developer").join("SKILL.md").is_file()
}

/// Fetch the latest published skills ref from the same installer users refresh
/// with, so "latest" always equals what a refresh would install. Best-effort:
/// None on any curl/network failure (doctor then reports "offline", never fails).
fn fetch_latest_skills_ref() -> Option<String> {
    let out = Command::new(crate::console::resolve_cmd("curl"))
        .args(["-fsSL", "--max-time", "6", SKILLS_INSTALL_URL])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_ref_from_installer(&String::from_utf8_lossy(&out.stdout))
}

/// Read-only currency report for the global Tina4 AI skills. Never writes; never
/// touches a project CLAUDE.md.
fn check_skills_currency() {
    println!();
    println!("  {}", "Tina4 AI skills (global)".bold());
    println!("  {}", "─".repeat(70));

    let skills_dir = match home_dir() {
        Some(h) => h.join(".claude").join("skills"),
        None => {
            println!("  {} could not resolve home directory", icon_warn().yellow());
            return;
        }
    };
    let dir_exists = skills_dir.is_dir();

    let known = [
        "tina4-developer-python", "tina4-developer-php", "tina4-developer-ruby",
        "tina4-developer-nodejs", "tina4-js", "tina4-maintainer",
    ];
    let present = if dir_exists {
        known.iter().filter(|s| skills_dir.join(s).join("SKILL.md").is_file()).count()
    } else {
        0
    };

    match classify_skills(dir_exists, read_installed_skills_ref(&skills_dir), fetch_latest_skills_ref()) {
        SkillsStatus::NotInstalled => {
            println!(
                "  {} skills not installed  {}  {}",
                icon_fail().red(), "->".dimmed(), format!("run: {}", SKILLS_INSTALL_CMD).yellow()
            );
        }
        SkillsStatus::Current(r) => {
            println!(
                "  {} {}",
                icon_ok().green(), format!("current - ref {} ({} skills installed)", r, present).cyan()
            );
        }
        SkillsStatus::Stale { installed, latest } => {
            println!(
                "  {} {}",
                icon_warn().yellow(),
                format!("update available - installed {}, latest {}", installed, latest).yellow()
            );
            println!("      {} {}", "refresh:".dimmed(), SKILLS_INSTALL_CMD.yellow());
        }
        SkillsStatus::InstalledUnknownLatest(i) => {
            println!(
                "  {} {}  {}",
                icon_info().blue(), format!("ref {} ({} skills)", i, present).cyan(),
                "could not check latest (offline)".dimmed()
            );
        }
        SkillsStatus::UnknownInstalled(latest) => {
            let tail = match latest {
                Some(l) => format!("version not recorded (latest is {}) - re-run install-skills to record + refresh", l),
                None => "version not recorded - re-run install-skills to record it".to_string(),
            };
            println!("  {} {} ({} skills present)", icon_info().blue(), tail.yellow(), present);
        }
    }

    if has_legacy_developer_skill(&skills_dir) {
        println!(
            "      {} legacy tina4-developer skill detected - {} removes it safely.",
            icon_warn().yellow(), SKILLS_INSTALL_CMD.yellow()
        );
    }

    // Make the safety guarantee explicit and visible: refreshing skills only ever
    // writes ~/.claude/skills. Neither doctor nor the refresh touches a project's CLAUDE.md.
    println!(
        "      {}",
        "refresh writes ~/.claude/skills only - it never changes a project's CLAUDE.md".dimmed()
    );
}

/// Read-only currency report for Codex personal skills in ~/.agents/skills.
fn check_codex_skills_currency() {
    println!();
    println!("  {}", "Tina4 AI skills (Codex)".bold());
    println!("  {}", "-".repeat(70));

    let skills_dir = match home_dir() {
        Some(h) => h.join(".agents").join("skills"),
        None => {
            println!("  {} could not resolve home directory", icon_warn().yellow());
            return;
        }
    };
    let known = [
        "tina4-developer-python", "tina4-developer-php", "tina4-developer-ruby",
        "tina4-developer-nodejs", "tina4-js", "tina4-maintainer",
    ];
    let present = if skills_dir.is_dir() {
        known.iter().filter(|s| skills_dir.join(s).join("SKILL.md").is_file()).count()
    } else {
        0
    };

    match classify_skills(skills_dir.is_dir(), read_installed_skills_ref(&skills_dir), fetch_latest_skills_ref()) {
        SkillsStatus::NotInstalled => println!(
            "  {} skills not installed  {}  {}",
            icon_fail().red(), "->".dimmed(), format!("run: {}", CODEX_SKILLS_INSTALL_CMD).yellow()
        ),
        SkillsStatus::Current(reference) => println!(
            "  {} {}",
            icon_ok().green(), format!("current - ref {} ({} skills installed)", reference, present).cyan()
        ),
        SkillsStatus::Stale { installed, latest } => {
            println!(
                "  {} {}",
                icon_warn().yellow(),
                format!("update available - installed {}, latest {}", installed, latest).yellow()
            );
            println!("      {} {}", "refresh:".dimmed(), CODEX_SKILLS_INSTALL_CMD.yellow());
        }
        SkillsStatus::InstalledUnknownLatest(reference) => println!(
            "  {} {}  {}",
            icon_info().blue(), format!("ref {} ({} skills)", reference, present).cyan(),
            "could not check latest (offline)".dimmed()
        ),
        SkillsStatus::UnknownInstalled(latest) => {
            let tail = latest.map_or_else(
                || "version not recorded - re-run the Codex installer".to_string(),
                |reference| format!("version not recorded (latest is {}) - re-run the Codex installer", reference),
            );
            println!("  {} {} ({} skills present)", icon_info().blue(), tail.yellow(), present);
        }
    }
    if has_legacy_developer_skill(&skills_dir) {
        println!(
            "      {} legacy tina4-developer skill detected - {} removes it safely.",
            icon_warn().yellow(), CODEX_SKILLS_INSTALL_CMD.yellow()
        );
    }
    println!(
        "      {}",
        "refresh writes ~/.agents/skills only - it never changes a project's AGENTS.md".dimmed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ref_from_installer_script() {
        let s = "set -euo pipefail\nref=\"${TINA4_SKILLS_REF:-3.13.73}\"\ndest=x";
        assert_eq!(parse_ref_from_installer(s), Some("3.13.73".to_string()));
    }

    #[test]
    fn parses_ref_from_powershell_form() {
        let s = "$ref = if ($env:TINA4_SKILLS_REF) { $env:TINA4_SKILLS_REF } else { \"3.13.73\" }";
        // The bash marker is absent here; parser returns None (doctor fetches the .sh form).
        assert_eq!(parse_ref_from_installer(s), None);
    }

    #[test]
    fn parse_ref_none_when_absent() {
        assert_eq!(parse_ref_from_installer("nothing to see"), None);
    }

    #[test]
    fn classify_not_installed_when_dir_missing() {
        assert_eq!(
            classify_skills(false, Some("3.13.73".into()), Some("3.13.73".into())),
            SkillsStatus::NotInstalled
        );
    }

    #[test]
    fn classify_current_on_match() {
        assert_eq!(
            classify_skills(true, Some("3.13.73".into()), Some("3.13.73".into())),
            SkillsStatus::Current("3.13.73".into())
        );
    }

    #[test]
    fn classify_stale_on_mismatch() {
        assert_eq!(
            classify_skills(true, Some("3.13.71".into()), Some("3.13.73".into())),
            SkillsStatus::Stale { installed: "3.13.71".into(), latest: "3.13.73".into() }
        );
    }

    #[test]
    fn classify_offline_when_latest_unknown() {
        assert_eq!(
            classify_skills(true, Some("3.13.73".into()), None),
            SkillsStatus::InstalledUnknownLatest("3.13.73".into())
        );
    }

    #[test]
    fn classify_unknown_when_no_marker() {
        assert_eq!(
            classify_skills(true, None, Some("3.13.73".into())),
            SkillsStatus::UnknownInstalled(Some("3.13.73".into()))
        );
    }
}
