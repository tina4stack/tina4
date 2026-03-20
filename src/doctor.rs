use colored::Colorize;
use std::process::Command;

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
                commands: &["composer"],
                version_flag: "--version",
            }),
        },
        ToolCheck {
            name: "Ruby",
            commands: &["ruby"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "bundler",
                commands: &["bundle"],
                version_flag: "--version",
            }),
        },
        ToolCheck {
            name: "Node.js",
            commands: &["node"],
            version_flag: "--version",
            pkg_manager: Some(PkgCheck {
                name: "npm",
                commands: &["npm"],
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
            "✓".green().to_string()
        } else {
            "✗".red().to_string()
        };
        let pkg_icon = if pkg_status {
            "✓".green().to_string()
        } else if tool.pkg_manager.is_some() {
            "✗".red().to_string()
        } else {
            "—".dimmed().to_string()
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

    // Check for tina4 language CLIs
    println!();
    println!("  {}", "Tina4 CLIs".bold());
    println!("  {}", "─".repeat(70));

    let clis = [
        ("tina4python", "Python"),
        ("tina4php", "PHP"),
        ("tina4ruby", "Ruby"),
        ("tina4nodejs", "Node.js"),
    ];

    for (cli, lang) in &clis {
        let found = which::which(cli).is_ok();
        let icon = if found {
            "✓".green().to_string()
        } else {
            "✗".red().to_string()
        };
        let status_text = if found {
            "installed".cyan().to_string()
        } else {
            "not found".dimmed().to_string()
        };
        println!("  {} {:<16} {:<12} {}", icon, cli, lang, status_text);
    }

    // Current project detection
    println!();
    match crate::detect::detect_language() {
        Some(info) => {
            println!(
                "  {} Current directory: {} project",
                "▶".green(),
                info.language.cyan()
            );
        }
        None => {
            println!(
                "  {} No Tina4 project detected in current directory",
                "ℹ".blue()
            );
        }
    }

    println!();
}

fn check_tool(commands: &[&str], version_flag: &str) -> (bool, String) {
    for cmd in commands {
        if let Ok(output) = Command::new(cmd)
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

fn extract_version_number(raw: &str) -> String {
    // Find the first thing that looks like a version number (digits.digits...)
    let first_line = raw.lines().next().unwrap_or("");
    for word in first_line.split_whitespace() {
        let trimmed = word.trim_start_matches('v');
        if trimmed.contains('.') && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return trimmed.to_string();
        }
    }
    first_line.trim().to_string()
}
