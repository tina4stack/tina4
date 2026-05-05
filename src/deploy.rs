// Tina4 deploy artefact scaffolding.
//
// `tina4 deploy <target>` writes the boilerplate file(s) needed to ship
// a Tina4 app to a particular environment. Today we cover four targets:
// docker, systemd, nginx, cpanel. Every template is baked into the
// binary so the command works air-gapped — no fetch, no network, no
// crate bloat from a templating engine. The templates are short and
// language-aware via the existing detect::detect_language helper.

use crate::console::{icon_fail, icon_info, icon_ok};
use crate::detect::{self, ProjectInfo};
use colored::Colorize;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy)]
pub enum Target {
    Docker,
    Systemd,
    Nginx,
    Cpanel,
}

impl Target {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "docker" => Some(Self::Docker),
            "systemd" => Some(Self::Systemd),
            "nginx" => Some(Self::Nginx),
            "cpanel" => Some(Self::Cpanel),
            _ => None,
        }
    }
}

/// Public entry point — invoked by `tina4 deploy <target>`.
pub fn run(target: &str, force: bool) {
    let Some(target) = Target::parse(target) else {
        eprintln!(
            "{} unknown deploy target: {}\n  valid targets: docker, systemd, nginx, cpanel",
            icon_fail().red(),
            target
        );
        std::process::exit(2);
    };

    let info = match detect::detect_language() {
        Some(info) => info,
        None => {
            eprintln!(
                "{} no Tina4 project detected in the current directory",
                icon_fail().red()
            );
            std::process::exit(1);
        }
    };

    let written = match target {
        Target::Docker => emit_docker(&info, force),
        Target::Systemd => emit_systemd(&info, force),
        Target::Nginx => emit_nginx(&info, force),
        Target::Cpanel => emit_cpanel(&info, force),
    };

    if written.is_empty() {
        println!(
            "{} nothing to write — every target file already exists. Re-run with {} to overwrite.",
            icon_info().yellow(),
            "--force".cyan()
        );
        return;
    }

    println!();
    println!("{} wrote:", icon_ok().green());
    for path in &written {
        println!("  • {}", path.cyan());
    }
    println!();
    print_next_steps(target);
}

// ── Targets ───────────────────────────────────────────────────────────

fn emit_docker(info: &ProjectInfo, force: bool) -> Vec<String> {
    let dockerfile = match info.language.as_str() {
        "python"   => DOCKERFILE_PYTHON,
        "php"      => DOCKERFILE_PHP,
        "ruby"     => DOCKERFILE_RUBY,
        "nodejs"   => DOCKERFILE_NODEJS,
        _          => DOCKERFILE_PYTHON,
    };
    let mut written = Vec::new();
    if write_if_absent("Dockerfile", dockerfile, force) {
        written.push("Dockerfile".to_string());
    }
    if write_if_absent(".dockerignore", DOCKERIGNORE, force) {
        written.push(".dockerignore".to_string());
    }
    written
}

fn emit_systemd(info: &ProjectInfo, force: bool) -> Vec<String> {
    let unit = SYSTEMD_UNIT
        .replace("{{LANGUAGE}}", &info.language)
        .replace("{{PROJECT}}", &project_name());
    let path = format!("deploy/tina4-{}.service", project_name());
    let mut written = Vec::new();
    if write_if_absent(&path, &unit, force) {
        written.push(path);
    }
    written
}

fn emit_nginx(_info: &ProjectInfo, force: bool) -> Vec<String> {
    let conf = NGINX_CONF.replace("{{PROJECT}}", &project_name());
    let path = format!("deploy/{}.nginx.conf", project_name());
    let mut written = Vec::new();
    if write_if_absent(&path, &conf, force) {
        written.push(path);
    }
    written
}

fn emit_cpanel(_info: &ProjectInfo, force: bool) -> Vec<String> {
    // cPanel deployments use Apache, so .htaccess is the single source
    // of truth for routing. A short README points users at what's left
    // (database creds, file permissions) since cPanel UI does the rest.
    let mut written = Vec::new();
    if write_if_absent(".htaccess", CPANEL_HTACCESS, force) {
        written.push(".htaccess".to_string());
    }
    if write_if_absent("deploy/CPANEL.md", CPANEL_README, force) {
        written.push("deploy/CPANEL.md".to_string());
    }
    written
}

// ── Helpers ───────────────────────────────────────────────────────────

fn project_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "tina4-app".to_string())
}

fn write_if_absent(path: &str, contents: &str, force: bool) -> bool {
    if Path::new(path).exists() && !force {
        return false;
    }
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Err(e) = fs::write(path, contents) {
        eprintln!("{} could not write {}: {}", icon_fail().red(), path, e);
        std::process::exit(1);
    }
    true
}

fn print_next_steps(target: Target) {
    match target {
        Target::Docker => {
            println!("Next:");
            println!("  {} review {} for language-specific bits", "•".dimmed(), "Dockerfile".cyan());
            println!("  {} {}                       # build image", "•".dimmed(), "docker build -t my-app .".cyan());
            println!("  {} {}              # run", "•".dimmed(), "docker run -p 7145:7145 my-app".cyan());
        }
        Target::Systemd => {
            println!("Next:");
            println!("  {} review the unit file in {}", "•".dimmed(), "deploy/".cyan());
            println!("  {} {}", "•".dimmed(), format!("sudo cp deploy/tina4-{}.service /etc/systemd/system/", project_name()).cyan());
            println!("  {} {}", "•".dimmed(), format!("sudo systemctl enable --now tina4-{}", project_name()).cyan());
        }
        Target::Nginx => {
            println!("Next:");
            println!("  {} review the server block in {}", "•".dimmed(), "deploy/".cyan());
            println!("  {} {}", "•".dimmed(), format!("sudo cp deploy/{}.nginx.conf /etc/nginx/sites-available/", project_name()).cyan());
            println!("  {} {}", "•".dimmed(), format!("sudo ln -s /etc/nginx/sites-available/{p}.nginx.conf /etc/nginx/sites-enabled/{p}.nginx.conf", p = project_name()).cyan());
            println!("  {} {}", "•".dimmed(), "sudo systemctl reload nginx".cyan());
        }
        Target::Cpanel => {
            println!("Next:");
            println!("  {} upload the project tree (or pull via git) into your cPanel account's web root", "•".dimmed());
            println!("  {} the {} drives clean URLs and SPA fallback", "•".dimmed(), ".htaccess".cyan());
            println!("  {} read {} for the rest", "•".dimmed(), "deploy/CPANEL.md".cyan());
        }
    }
}

// ── Templates ─────────────────────────────────────────────────────────

const DOCKERFILE_PYTHON: &str = include_str!("../templates/deploy/Dockerfile.python");
const DOCKERFILE_PHP: &str = include_str!("../templates/deploy/Dockerfile.php");
const DOCKERFILE_RUBY: &str = include_str!("../templates/deploy/Dockerfile.ruby");
const DOCKERFILE_NODEJS: &str = include_str!("../templates/deploy/Dockerfile.nodejs");
const DOCKERIGNORE: &str = include_str!("../templates/deploy/dockerignore");
const SYSTEMD_UNIT: &str = include_str!("../templates/deploy/systemd.service");
const NGINX_CONF: &str = include_str!("../templates/deploy/nginx.conf");
const CPANEL_HTACCESS: &str = include_str!("../templates/deploy/cpanel.htaccess");
const CPANEL_README: &str = include_str!("../templates/deploy/CPANEL.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parse_known() {
        assert!(matches!(Target::parse("docker"), Some(Target::Docker)));
        assert!(matches!(Target::parse("DOCKER"), Some(Target::Docker)));
        assert!(matches!(Target::parse("systemd"), Some(Target::Systemd)));
        assert!(matches!(Target::parse("nginx"), Some(Target::Nginx)));
        assert!(matches!(Target::parse("cpanel"), Some(Target::Cpanel)));
    }

    #[test]
    fn target_parse_unknown() {
        assert!(Target::parse("kubernetes").is_none());
        assert!(Target::parse("").is_none());
    }
}
