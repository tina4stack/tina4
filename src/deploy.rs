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

    // ── Dockerfile CMD contract ───────────────────────────────────────────
    //
    // These templates are baked in with include_str!, so nothing at build time
    // notices when a framework's entry point moves. That is exactly how
    // `tina4 deploy docker` shipped a Python image whose CMD could never
    // execute: tina4_python/cli.py (a MODULE, where `python -m` is valid)
    // became tina4_python/cli/ (a PACKAGE with no __main__.py) in the v3
    // restructure, and the hard-coded string never followed. The container died
    // instantly with "'tina4_python.cli' is a package and cannot be directly
    // executed" and no gate anywhere caught it.
    //
    // Each test below encodes one way a generated CMD can be un-runnable. They
    // are pure string assertions over templates we own -- no dependency, no
    // double. The docker-build job in CI is the end-to-end partner to these.

    fn all_dockerfiles() -> [(&'static str, &'static str); 4] {
        [
            ("python", DOCKERFILE_PYTHON),
            ("php", DOCKERFILE_PHP),
            ("ruby", DOCKERFILE_RUBY),
            ("nodejs", DOCKERFILE_NODEJS),
        ]
    }

    fn cmd_line(dockerfile: &str) -> &str {
        dockerfile
            .lines()
            .find(|l| l.starts_with("CMD "))
            .expect("every Dockerfile template must declare a CMD")
    }

    /// `python -m <pkg>` only works when <pkg> is a module, or a package with a
    /// __main__.py. Naming a package without one is silently accepted at
    /// generate time and fails at container start, so ban the form outright.
    #[test]
    fn no_dash_m_on_a_package() {
        for (lang, body) in all_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                !cmd.contains("\"-m\""),
                "{lang}: CMD uses `python -m <pkg>`, which cannot execute a \
                 package without __main__.py. Use the console script instead: {cmd}"
            );
        }
    }

    /// tsx is a devDependency. The production stage installs with
    /// `npm ci --omit=dev`, which strips it, so `npx tsx` would have to fetch
    /// tsx off the network at container start (and fails when there isn't any).
    #[test]
    fn no_dev_only_runner_in_production_cmd() {
        for (lang, body) in all_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                !cmd.contains("\"tsx\""),
                "{lang}: CMD invokes tsx, a devDependency stripped by the \
                 production install: {cmd}"
            );
        }
    }

    /// A CMD must name the entry point the framework actually publishes. These
    /// are the verified-real ones: the tina4python console script
    /// ([project.scripts]), composer's vendor/bin/tina4php symlink, the
    /// tina4ruby gem exe, and the tina4nodejs package `bin`.
    #[test]
    fn cmd_names_a_real_published_entry_point() {
        let expected = [
            ("python", "tina4python"),
            ("php", "vendor/bin/tina4php"),
            ("ruby", "tina4ruby"),
            ("nodejs", "tina4nodejs"),
        ];
        for (lang, entry) in expected {
            let body = all_dockerfiles()
                .into_iter()
                .find(|(l, _)| *l == lang)
                .map(|(_, b)| b)
                .unwrap();
            let cmd = cmd_line(body);
            assert!(
                cmd.contains(entry),
                "{lang}: CMD does not name the published entry point `{entry}`: {cmd}"
            );
        }
    }

    /// A deploy image is production. Every CMD must ask for it, or the server
    /// boots in dev mode (watchers, dev toolbar, no production HTTP server).
    #[test]
    fn every_cmd_requests_production() {
        for (lang, body) in all_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                cmd.contains("--production"),
                "{lang}: CMD never requests production mode: {cmd}"
            );
        }
    }

    /// A base image below the framework's own declared floor builds CLEANLY and
    /// dies at container start. That is how `tina4 deploy docker` shipped a Node
    /// image on node:20 while tina4-nodejs declares `engines.node >=22` and
    /// imports the built-in `node:sqlite` (added in 22.5): npm downgrades an
    /// engines mismatch to a warning, the build went green, and the container
    /// exited immediately with ERR_UNKNOWN_BUILTIN_MODULE.
    ///
    /// These floors mirror the frameworks' published manifests -- pyproject
    /// `requires-python`, composer `require.php`, the gemspec
    /// `required_ruby_version`, and package.json `engines.node`. They are
    /// duplicated here because the CLI ships independently of all four and
    /// cannot read their manifests at build time. Raising a framework floor
    /// means raising the number here too, and this test is what makes that
    /// failure loud instead of silent.
    #[test]
    fn base_image_meets_the_framework_floor() {
        let floors = [
            ("python", "FROM python:", 3, 12),
            ("php", "FROM php:", 8, 2),
            ("ruby", "FROM ruby:", 3, 1),
            ("nodejs", "FROM node:", 22, 0),
        ];
        for (lang, prefix, min_major, min_minor) in floors {
            let body = all_dockerfiles()
                .into_iter()
                .find(|(l, _)| *l == lang)
                .map(|(_, b)| b)
                .unwrap();
            let mut checked = 0;
            for line in body.lines().filter(|l| l.starts_with(prefix)) {
                let tag = &line[prefix.len()..];
                // "3.12-slim" -> 3.12 ; "24-alpine" -> 24 (minor absent = 0)
                let version: String = tag
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let mut parts = version.split('.');
                let major: u32 = parts.next().unwrap_or("").parse().unwrap_or_else(|_| {
                    panic!("{lang}: cannot read a version out of `{line}`")
                });
                let minor: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
                assert!(
                    (major, minor) >= (min_major, min_minor),
                    "{lang}: base image pins {major}.{minor}, below the framework's \
                     declared minimum {min_major}.{min_minor}. The image will build \
                     and then fail at container start: {line}"
                );
                checked += 1;
            }
            assert!(
                checked > 0,
                "{lang}: no `{prefix}` line found, so the floor was never checked. \
                 Did the template switch base image?"
            );
        }
    }

    /// The frameworks refuse to boot unless launched by the `tina4` client,
    /// which is not in the image. TINA4_OVERRIDE_CLIENT is the documented
    /// escape hatch, so every template has to set it or the container cannot
    /// start at all.
    #[test]
    fn every_template_sets_override_client() {
        for (lang, body) in all_dockerfiles() {
            assert!(
                body.contains("TINA4_OVERRIDE_CLIENT=true"),
                "{lang}: template never sets TINA4_OVERRIDE_CLIENT=true, so the \
                 framework will refuse to boot without the tina4 client"
            );
        }
    }
}
