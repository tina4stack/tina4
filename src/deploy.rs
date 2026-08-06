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

/// PHP runtime flavour for `deploy docker`.
///
/// PHP is the only language with a genuine choice here, because it is the only
/// one where the process model is a deployment decision rather than a property
/// of the runtime: cli keeps the framework's own forking server, fpm gives a
/// fresh process per request behind nginx, and swoole keeps the app resident.
/// The other three have one sensible answer each, so `--runtime` is refused
/// there rather than silently ignored.
#[derive(Clone, Copy, PartialEq)]
pub enum PhpRuntime {
    Cli,
    Fpm,
    Swoole,
}

impl PhpRuntime {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cli" | "default" | "builtin" => Some(Self::Cli),
            "fpm" | "php-fpm" | "fpm-nginx" => Some(Self::Fpm),
            "swoole" | "openswoole" => Some(Self::Swoole),
            _ => None,
        }
    }
}

/// Public entry point — invoked by `tina4 deploy <target>`.
pub fn run(target: &str, runtime: Option<&str>, force: bool) {
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

    // Resolve --runtime before any file is written, so a typo fails with a
    // named error instead of quietly producing the default image.
    let php_runtime = match runtime {
        None => PhpRuntime::Cli,
        Some(raw) => {
            if info.language != "php" {
                eprintln!(
                    "{} --runtime applies to PHP only; {} has one deployment runtime",
                    icon_fail().red(),
                    info.language
                );
                std::process::exit(2);
            }
            match PhpRuntime::parse(raw) {
                Some(r) => r,
                None => {
                    eprintln!(
                        "{} unknown PHP runtime: {}\n  valid runtimes: cli, fpm, swoole",
                        icon_fail().red(),
                        raw
                    );
                    std::process::exit(2);
                }
            }
        }
    };

    let written = match target {
        Target::Docker => emit_docker(&info, php_runtime, force),
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

fn emit_docker(info: &ProjectInfo, php_runtime: PhpRuntime, force: bool) -> Vec<String> {
    let dockerfile = match info.language.as_str() {
        "python"   => DOCKERFILE_PYTHON,
        "php"      => match php_runtime {
            PhpRuntime::Cli    => DOCKERFILE_PHP,
            PhpRuntime::Fpm    => DOCKERFILE_PHP_FPM,
            PhpRuntime::Swoole => DOCKERFILE_PHP_SWOOLE,
        },
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

    // Each PHP runtime needs its own companion files, and the Dockerfile COPYs
    // them: emitting the Dockerfile without them produces an image that fails
    // to build. They are written together or not at all.
    if info.language == "php" {
        match php_runtime {
            PhpRuntime::Cli => {}
            PhpRuntime::Swoole => {
                if write_if_absent("server.php", SERVER_PHP_SWOOLE, force) {
                    written.push("server.php".to_string());
                }
            }
            PhpRuntime::Fpm => {
                if write_if_absent("nginx.fpm.conf", NGINX_FPM_CONF, force) {
                    written.push("nginx.fpm.conf".to_string());
                }
                if write_if_absent("docker-entrypoint.fpm.sh", ENTRYPOINT_FPM, force) {
                    written.push("docker-entrypoint.fpm.sh".to_string());
                }
            }
        }
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
const DOCKERFILE_PHP_FPM: &str = include_str!("../templates/deploy/Dockerfile.php.fpm");
const DOCKERFILE_PHP_SWOOLE: &str = include_str!("../templates/deploy/Dockerfile.php.swoole");
const SERVER_PHP_SWOOLE: &str = include_str!("../templates/deploy/server.php.swoole");
const NGINX_FPM_CONF: &str = include_str!("../templates/deploy/nginx.fpm.conf");
const ENTRYPOINT_FPM: &str = include_str!("../templates/deploy/docker-entrypoint.fpm.sh");
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

    fn all_dockerfiles() -> [(&'static str, &'static str); 6] {
        [
            ("python", DOCKERFILE_PYTHON),
            ("php", DOCKERFILE_PHP),
            ("php-fpm", DOCKERFILE_PHP_FPM),
            ("php-swoole", DOCKERFILE_PHP_SWOOLE),
            ("ruby", DOCKERFILE_RUBY),
            ("nodejs", DOCKERFILE_NODEJS),
        ]
    }

    // ── PHP runtime selection ─────────────────────────────────────────────

    /// CLAUDE.md's version header must match Cargo.toml.
    ///
    /// This is the CLI's missing half of a gate the PHP framework already has
    /// (VersionConsistencyTest). It was missing, and it had drifted by four
    /// releases: Cargo.toml said 3.8.67 while CLAUDE.md still said 3.8.63.
    /// CLAUDE.md is what every AI assistant reads first, so a stale version
    /// there is a wrong answer repeated to every user who asks.
    #[test]
    fn claude_md_version_header_matches_the_crate() {
        let claude_md = include_str!("../CLAUDE.md");
        let header = claude_md
            .lines()
            .find(|l| l.starts_with("Version "))
            .expect("CLAUDE.md must carry a `Version X.Y.Z` header");
        let declared = header
            .split_whitespace()
            .nth(1)
            .expect("the Version header must name a version");
        assert_eq!(
            declared,
            env!("CARGO_PKG_VERSION"),
            "CLAUDE.md header disagrees with Cargo.toml -- bump both with the release"
        );
    }

    #[test]
    fn php_runtime_parse_known() {
        assert!(matches!(PhpRuntime::parse("cli"), Some(PhpRuntime::Cli)));
        assert!(matches!(PhpRuntime::parse("FPM"), Some(PhpRuntime::Fpm)));
        assert!(matches!(PhpRuntime::parse("php-fpm"), Some(PhpRuntime::Fpm)));
        assert!(matches!(PhpRuntime::parse("swoole"), Some(PhpRuntime::Swoole)));
        assert!(matches!(PhpRuntime::parse("openswoole"), Some(PhpRuntime::Swoole)));
    }

    #[test]
    fn php_runtime_parse_unknown() {
        assert!(PhpRuntime::parse("roadrunner").is_none());
        assert!(PhpRuntime::parse("frankenphp").is_none());
        assert!(PhpRuntime::parse("").is_none());
    }

    /// The three PHP images must be genuinely different programs. If two ever
    /// collapse to the same bytes, `--runtime` is a lie that still exits 0.
    #[test]
    fn the_three_php_images_are_distinct() {
        assert_ne!(DOCKERFILE_PHP, DOCKERFILE_PHP_FPM);
        assert_ne!(DOCKERFILE_PHP, DOCKERFILE_PHP_SWOOLE);
        assert_ne!(DOCKERFILE_PHP_FPM, DOCKERFILE_PHP_SWOOLE);
    }

    /// Every file a Dockerfile COPYs from the build context must be a file this
    /// command actually writes. The swoole image COPYs server.php and the fpm
    /// image COPYs two companions; emitting the Dockerfile without them yields
    /// an image that cannot build, which is the same class of defect as the
    /// un-runnable Python CMD these tests were written for.
    #[test]
    fn every_copied_companion_file_is_one_we_emit() {
        assert!(DOCKERFILE_PHP_SWOOLE.contains("COPY server.php"));
        assert!(!SERVER_PHP_SWOOLE.is_empty());

        assert!(DOCKERFILE_PHP_FPM.contains("COPY nginx.fpm.conf"));
        assert!(DOCKERFILE_PHP_FPM.contains("COPY docker-entrypoint.fpm.sh"));
        assert!(!NGINX_FPM_CONF.is_empty());
        assert!(!ENTRYPOINT_FPM.is_empty());
    }

    /// The swoole image must not launch the framework's own server: the whole
    /// point is that Swoole IS the server. A `tina4 serve` CMD here would bind
    /// the port with the built-in server and never start Swoole at all.
    #[test]
    fn the_swoole_image_starts_swoole_not_the_builtin_server() {
        let cmd = cmd_line(DOCKERFILE_PHP_SWOOLE);
        assert!(cmd.contains("server.php"), "swoole CMD must run the swoole entry point: {cmd}");
        assert!(!cmd.contains("tina4"), "swoole CMD must not launch the built-in server: {cmd}");
    }

    /// nginx must be PID 1 in the fpm image so `docker stop` is a clean
    /// shutdown rather than a 10s wait then SIGKILL.
    #[test]
    fn the_fpm_entrypoint_execs_nginx_into_pid_one() {
        assert!(
            ENTRYPOINT_FPM.contains("exec nginx"),
            "nginx must be exec'd, not backgrounded, or signals never reach it"
        );
    }

    /// The entrypoint runs as the container's ENTRYPOINT on an image whose
    /// /bin/sh is dash. bash-isms there fail at container start, and the two
    /// below fail SILENTLY: dash treats /dev/tcp as an ordinary missing path
    /// and has no `pipefail`, so a readiness loop built on them falls straight
    /// through and defeats its own purpose. This repo has shipped exactly that
    /// bug before, in install-skills.sh.
    #[test]
    fn the_fpm_entrypoint_is_posix_sh() {
        assert!(ENTRYPOINT_FPM.starts_with("#!/bin/sh"));
        let code = shell_code_only(ENTRYPOINT_FPM);
        assert!(!code.contains("pipefail"), "dash has no `set -o pipefail`");
        assert!(!code.contains("/dev/tcp"), "/dev/tcp is a bash feature; dash silently skips it");
        assert!(!code.contains("=("), "dash has no arrays");
    }

    /// nginx must hand unknown paths to index.php. Tina4 is a front-controller
    /// framework: without try_files, every route but "/" returns nginx's own
    /// 404 and the router never runs.
    #[test]
    fn the_fpm_nginx_config_routes_through_the_front_controller() {
        assert!(NGINX_FPM_CONF.contains("try_files"));
        assert!(NGINX_FPM_CONF.contains("index.php"));
        assert!(NGINX_FPM_CONF.contains("fastcgi_pass"));
    }

    /// A misconfigured location here serves .env over HTTP, which leaks
    /// TINA4_SECRET and every database credential the app has.
    #[test]
    fn the_fpm_nginx_config_refuses_dotfiles_and_vendor() {
        assert!(NGINX_FPM_CONF.contains("location ~ /\\."), "dotfiles (.env, .git) must be denied");
        assert!(NGINX_FPM_CONF.contains("vendor"), "the dependency tree must not be web-served");
    }

    /// The swoole entry script must not call a procedural swoole_*() helper
    /// unguarded.
    ///
    /// This is not hypothetical tidiness. OpenSwoole 22+ REMOVED those aliases:
    /// on openswoole 26.2.0 swoole_cpu_num() and swoole_version() are both
    /// gone, while mainline Swoole still has them. An earlier version of this
    /// template called swoole_cpu_num() for the worker count, and the result
    /// was the nastiest possible shape of failure - the image BUILT clean and
    /// the container then exited 255 on startup with "Call to undefined
    /// function swoole_cpu_num()". Measured, not imagined.
    ///
    /// A guarded call is fine and is what the template does now, so the check
    /// is for a call OUTSIDE a function_exists() guard.
    #[test]
    fn the_swoole_entry_script_guards_every_procedural_helper() {
        let code = php_code_only(SERVER_PHP_SWOOLE);
        for helper in ["swoole_cpu_num", "swoole_version", "swoole_get_local_ip"] {
            let calls = code.matches(&format!("{helper}(")).count();
            if calls == 0 {
                continue;
            }
            let guards = code.matches(&format!("function_exists('{helper}')")).count();
            assert!(
                guards >= 1,
                "{helper}() is called but never guarded with function_exists();                  OpenSwoole 22+ removed it and the container will exit 255 at startup"
            );
        }
    }

    /// The worker count must resolve on BOTH builds, so both spellings have to
    /// be present: OpenSwoole\Util for openswoole, the procedural helper for
    /// mainline Swoole. One alone silently excludes half the users.
    #[test]
    fn the_swoole_entry_script_resolves_workers_on_either_extension() {
        assert!(
            php_code_only(SERVER_PHP_SWOOLE).contains(r"OpenSwoole\Util"),
            r"no OpenSwoole\Util path: the worker count cannot resolve on openswoole"
        );
        assert!(
            php_code_only(SERVER_PHP_SWOOLE).contains("swoole_cpu_num"),
            "no swoole_cpu_num path: the worker count cannot resolve on mainline Swoole"
        );
    }

    /// A resident Swoole worker never discards process state between requests,
    /// so debug mode's ever-growing static arrays are a guaranteed leak.
    #[test]
    fn the_swoole_image_pins_debug_off() {
        assert!(DOCKERFILE_PHP_SWOOLE.contains("TINA4_DEBUG=false"));
    }

    /// A silently-missing extension would produce an image whose CMD dies at
    /// container start, so the build must verify the extension loaded.
    #[test]
    fn the_swoole_image_fails_the_build_when_the_extension_is_absent() {
        assert!(
            DOCKERFILE_PHP_SWOOLE.contains("php -m | grep -qi '^openswoole$'"),
            "the build must assert openswoole actually loaded"
        );
    }

    /// The images whose server IS the framework's own, launched by the tina4
    /// CLI. The PHP fpm and swoole variants are deliberately NOT here: their
    /// whole purpose is that nginx+php-fpm or Swoole is the server instead, so
    /// a `tina4 serve` CMD in those would bind the port with the built-in
    /// server and the chosen runtime would never start. They keep their
    /// guarantees through the runtime-agnostic tests below.
    fn cli_launched_dockerfiles() -> [(&'static str, &'static str); 4] {
        [
            ("python", DOCKERFILE_PYTHON),
            ("php", DOCKERFILE_PHP),
            ("ruby", DOCKERFILE_RUBY),
            ("nodejs", DOCKERFILE_NODEJS),
        ]
    }

    /// Comment-stripped view of a shell script, for tests that must assert
    /// about CODE rather than prose. Writing "dash has no `set -o pipefail`"
    /// in a comment is not the same as USING pipefail, and a naive substring
    /// check cannot tell the difference - it flagged this very file's own
    /// explanatory comment.
    fn shell_code_only(script: &str) -> String {
        script
            .lines()
            .map(|l| match l.find('#') {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Comment-stripped view of a PHP file, for the same reason
    /// shell_code_only() exists: a test about what the code CALLS must not read
    /// what the comments SAY. The guard test below flagged its own template's
    /// docblock - the sentence explaining that swoole_version() was removed
    /// counted as a call to it.
    ///
    /// Handles `/* ... */` blocks (which is what a docblock is) and `//` line
    /// comments. Not a PHP parser and does not need to be: these templates are
    /// ours, and no string literal in them contains a comment opener.
    fn php_code_only(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let bytes: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == '/' && bytes[i + 1] == '*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
            if i + 1 < bytes.len() && bytes[i] == '/' && bytes[i + 1] == '/' {
                while i < bytes.len() && bytes[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
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

    /// Every image installs the tina4 CLI and launches through it, so every
    /// CMD must invoke `tina4` -- one launcher, four languages. This replaced
    /// four per-language entry points (tina4python, vendor/bin/tina4php,
    /// tina4ruby, npx tina4nodejs); `npx tina4nodejs` in particular exited 0
    /// and served nothing inside a container, which a uniform launcher makes
    /// impossible to repeat per-language.
    #[test]
    fn every_cmd_launches_through_the_tina4_cli() {
        for (lang, body) in cli_launched_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                cmd.contains("\"tina4\""),
                "{lang}: CMD does not launch through the tina4 CLI: {cmd}"
            );
            assert!(
                body.contains("COPY --from=tina4cli"),
                "{lang}: CMD calls tina4 but the template never copies the \
                 binary in, so the image has no launcher at all"
            );
        }
    }

    /// The CLI's release tags are v-PREFIXED (`v3.8.63`), so the image it
    /// publishes is `tina4-cli:v3.8.63`. Every template pinned the bare
    /// `3.8.63`, which is a 404 on GHCR -- so `tina4 deploy docker` emitted a
    /// Dockerfile that could not even pull its first stage. Nothing caught it
    /// because the generator tests only ever read the template text; the tag
    /// was never resolved against a registry, and it never will be in a unit
    /// test. This asserts the shape instead: the pin must look like the tags
    /// the CLI actually cuts.
    #[test]
    fn the_cli_image_is_pinned_to_a_v_prefixed_tag() {
        for (lang, body) in all_dockerfiles() {
            let pin = body
                .lines()
                .find(|l| l.starts_with("ARG TINA4_CLI_IMAGE="))
                .unwrap_or_else(|| panic!("{lang}: no TINA4_CLI_IMAGE pin"))
                .trim_start_matches("ARG TINA4_CLI_IMAGE=");
            let tag = pin.rsplit_once(':').map(|(_, t)| t).unwrap_or_else(|| {
                panic!("{lang}: CLI image {pin} carries no tag -- an untagged \
                        FROM silently means :latest, which moves under you")
            });
            assert!(
                tag.starts_with('v') && tag[1..].starts_with(|c: char| c.is_ascii_digit()),
                "{lang}: CLI image pinned to {tag:?}, but the tina4 CLI cuts \
                 v-prefixed tags (v3.8.63). A bare version is a 404 on GHCR \
                 and the build fails on its FIRST line."
            );
        }
    }

    /// npx resolves through the network and, in a container, exited 0 having
    /// served nothing -- a silent no-op that looks like success. Ban it.
    #[test]
    fn no_npx_in_a_production_cmd() {
        for (lang, body) in all_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                !cmd.contains("\"npx\""),
                "{lang}: CMD invokes npx, which exited 0 without serving \
                 anything in a container: {cmd}"
            );
        }
    }

    /// A deploy image is production. Every CMD must ask for it, or the server
    /// boots in dev mode (watchers, dev toolbar, no production HTTP server).
    #[test]
    fn every_cmd_requests_production() {
        for (lang, body) in cli_launched_dockerfiles() {
            let cmd = cmd_line(body);
            assert!(
                cmd.contains("--production"),
                "{lang}: CMD never requests production mode: {cmd}"
            );
        }
    }

    /// The runtime-agnostic half of the production guarantee, and the reason
    /// scoping the test above is not a loophole. `--production` selects a
    /// production HTTP SERVER, which is meaningless in an image where nginx or
    /// Swoole already is the server. What must hold everywhere is that the
    /// FRAMEWORK is not in dev mode - watchers, dev toolbar, the /__dev
    /// dashboard and its unbounded static request log. Every image states it.
    #[test]
    fn every_image_pins_the_framework_out_of_dev_mode() {
        for (lang, body) in all_dockerfiles() {
            assert!(
                body.contains("TINA4_DEBUG=false"),
                "{lang}: image never pins TINA4_DEBUG=false, so it can boot in dev mode"
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

    /// A TypeScript project must be COMPILED in the image, not transpiled at
    /// run time. Without a build step the Node container ran `npx tsx app.ts`:
    /// five processes (npm exec -> tsx -> node -> esbuild service) where the
    /// other three frameworks run one, and -- because `npm ci --omit=dev`
    /// strips tsx -- npx FETCHED the transpiler over the network at container
    /// start, so an air-gapped host never boots at all.
    #[test]
    fn nodejs_image_compiles_ahead_of_time() {
        let body = all_dockerfiles()
            .into_iter()
            .find(|(l, _)| *l == "nodejs")
            .map(|(_, b)| b)
            .unwrap();
        assert!(
            body.contains("tsc"),
            "nodejs: no TypeScript build step, so the image must transpile at \
             run time"
        );
        assert!(
            body.contains("/app/dist"),
            "nodejs: builds but never copies dist/ into the runtime stage, so \
             the compiled output does not ship"
        );
        assert!(
            body.contains("npm prune --omit=dev"),
            "nodejs: dev dependencies (typescript, tsx) would ship to production"
        );
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
