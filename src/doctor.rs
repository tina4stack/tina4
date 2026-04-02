use crate::console::{icon_ok, icon_fail, icon_play, icon_info, icon_warn};
use colored::Colorize;
use std::path::Path;
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
        CliCheck { name: "tina4php",    lang: "PHP",     install_cmd: "composer global require tina4/tina4php" },
        CliCheck { name: "tina4ruby",   lang: "Ruby",    install_cmd: "gem install tina4ruby" },
        CliCheck { name: "tina4nodejs", lang: "Node.js", install_cmd: "npm install -g tina4nodejs" },
    ];

    for cli in &clis {
        // Check global PATH first, then project-local paths
        let local_paths: &[&str] = match cli.name {
            "tina4php" => &["vendor/bin/tina4php", "bin/tina4php"],
            "tina4python" => &[".venv/bin/tina4python", "venv/bin/tina4python"],
            "tina4ruby" => &["bin/tina4ruby", "exe/tina4ruby"],
            "tina4nodejs" => &["node_modules/.bin/tina4nodejs", "packages/cli/src/bin.ts"],
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

    // --- Port availability ---
    println!();
    println!("  {}", "Ports".bold());
    println!("  {}", "─".repeat(70));

    let ports = [
        (7145u16, "Python"),
        (7146u16, "PHP"),
        (7147u16, "Ruby"),
        (7148u16, "Node.js"),
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
