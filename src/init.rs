use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Run the full init flow: check runtime, check package manager, scaffold, install deps.
pub fn run(lang: &str, path: &str) {
    let lang_norm = lang.to_lowercase();

    match lang_norm.as_str() {
        "python" | "py" => init_project("python", path),
        "php" => init_project("php", path),
        "ruby" | "rb" => init_project("ruby", path),
        "nodejs" | "node" | "js" | "typescript" | "ts" => init_project("nodejs", path),
        _ => {
            eprintln!(
                "{} Unknown language: {}. Use: python, php, ruby, nodejs",
                "✗".red(),
                lang
            );
            std::process::exit(1);
        }
    }
}

fn init_project(language: &str, path: &str) {
    let abs_path = to_absolute(path);
    let abs = abs_path.as_str();

    println!(
        "\n{} Initialising {} project at {}",
        "▶".green(),
        language.cyan(),
        abs.cyan()
    );

    // Step 1 — Check / install language runtime
    check_runtime(language);

    // Step 2 — Check / install package manager
    check_package_manager(language);

    // Step 3 — Scaffold project
    scaffold(language, abs);

    // Step 4 — Install framework package
    install_deps(language, abs);

    // Step 5 — Summary
    let port = default_port(language);
    println!();
    println!(
        "{} Tina4 project created at {}",
        "✓".green(),
        abs.cyan()
    );
    println!();
    println!("  cd {}", abs);
    println!("  tina4 serve");
    println!();
    println!(
        "  Your app will run at http://localhost:{} ({} default)",
        port, language
    );
    println!();
}

// ── Helpers ──────────────────────────────────────────────────────

fn to_absolute(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p).to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string())
    }
}

fn default_port(language: &str) -> u16 {
    match language {
        "python" => 7145,
        "php" => 7146,
        "ruby" => 7147,
        "nodejs" => 7148,
        _ => 7145,
    }
}

fn cmd_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn run_cmd(cmd: &str, args: &[&str]) -> bool {
    println!(
        "  {} Running: {} {}",
        "▶".green(),
        cmd,
        args.join(" ")
    );
    match Command::new(cmd).args(args).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "  {} Command failed (exit {:?})",
                "✗".red(),
                s.code()
            );
            false
        }
        Err(e) => {
            eprintln!("  {} Failed to run {}: {}", "✗".red(), cmd, e);
            false
        }
    }
}

fn run_cmd_in(dir: &str, cmd: &str, args: &[&str]) -> bool {
    println!(
        "  {} Running: {} {} (in {})",
        "▶".green(),
        cmd,
        args.join(" "),
        dir
    );
    match Command::new(cmd).args(args).current_dir(dir).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "  {} Command failed (exit {:?}). You can run it manually:\n      cd {} && {} {}",
                "✗".red(),
                s.code(),
                dir,
                cmd,
                args.join(" ")
            );
            false
        }
        Err(e) => {
            eprintln!(
                "  {} Failed to run {}: {}. You can run it manually:\n      cd {} && {} {}",
                "✗".red(),
                cmd,
                e,
                dir,
                cmd,
                args.join(" ")
            );
            false
        }
    }
}

// ── Step 1: Runtime ──────────────────────────────────────────────

fn check_runtime(language: &str) {
    println!("\n{} Checking {} runtime...", "▶".green(), language.cyan());

    match language {
        "python" => {
            if cmd_exists("python3") {
                println!("  {} python3 found", "✓".green());
            } else if cmd_exists("python") {
                println!("  {} python found", "✓".green());
            } else {
                println!("  {} python3 not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    run_cmd("brew", &["install", "python"]);
                } else {
                    eprintln!(
                        "  {} Please install Python 3: https://www.python.org/downloads/",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "php" => {
            if cmd_exists("php") {
                println!("  {} php found", "✓".green());
            } else {
                println!("  {} php not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    run_cmd("brew", &["install", "php"]);
                } else {
                    eprintln!(
                        "  {} Please install PHP: https://www.php.net/downloads",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "ruby" => {
            if cmd_exists("ruby") {
                println!("  {} ruby found", "✓".green());
            } else {
                println!("  {} ruby not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    run_cmd("brew", &["install", "ruby"]);
                } else {
                    eprintln!(
                        "  {} Please install Ruby: https://www.ruby-lang.org/en/downloads/",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "nodejs" => {
            if cmd_exists("node") {
                println!("  {} node found", "✓".green());
            } else {
                println!("  {} node not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    run_cmd("brew", &["install", "node"]);
                } else {
                    eprintln!(
                        "  {} Please install Node.js: https://nodejs.org/",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        _ => {}
    }
}

// ── Step 2: Package manager ──────────────────────────────────────

fn check_package_manager(language: &str) {
    println!(
        "\n{} Checking package manager...",
        "▶".green()
    );

    match language {
        "python" => {
            if cmd_exists("uv") {
                println!("  {} uv found", "✓".green());
            } else {
                println!("  {} uv not found — installing", "⚠".yellow());
                let ok = run_cmd(
                    "sh",
                    &["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"],
                );
                if !ok {
                    eprintln!(
                        "  {} Install uv manually: curl -LsSf https://astral.sh/uv/install.sh | sh",
                        "✗".red()
                    );
                }
            }
        }
        "php" => {
            if cmd_exists("composer") {
                println!("  {} composer found", "✓".green());
            } else {
                println!("  {} composer not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    run_cmd("brew", &["install", "composer"]);
                } else {
                    eprintln!(
                        "  {} Install Composer: https://getcomposer.org/download/",
                        "✗".red()
                    );
                }
            }
        }
        "ruby" => {
            if cmd_exists("bundle") {
                println!("  {} bundler found", "✓".green());
            } else {
                println!("  {} bundler not found — installing via gem", "⚠".yellow());
                run_cmd("gem", &["install", "bundler"]);
            }
        }
        "nodejs" => {
            if cmd_exists("npm") {
                println!("  {} npm found", "✓".green());
            } else {
                eprintln!(
                    "  {} npm not found — it should come with Node.js. Reinstall Node.",
                    "✗".red()
                );
            }
        }
        _ => {}
    }
}

// ── Step 3: Scaffold ─────────────────────────────────────────────

fn scaffold(language: &str, path: &str) {
    println!("\n{} Creating project scaffold...", "▶".green());

    let p = Path::new(path);
    if p.exists() {
        eprintln!(
            "{} Directory already exists: {}",
            "✗".red(),
            path
        );
        std::process::exit(1);
    }

    // Common directories
    let dirs = [
        "",
        "src",
        "src/routes",
        "src/orm",
        "src/templates",
        "src/public",
        "src/public/css",
        "src/scss",
        "migrations",
        "logs",
    ];
    for d in &dirs {
        let full = if d.is_empty() {
            p.to_path_buf()
        } else {
            p.join(d)
        };
        fs::create_dir_all(&full).unwrap_or_else(|e| {
            eprintln!(
                "{} Failed to create {}: {}",
                "✗".red(),
                full.display(),
                e
            );
            std::process::exit(1);
        });
    }

    // .env
    write_file(
        p,
        ".env",
        "TINA4_DEBUG_LEVEL=ALL\n",
    );

    // .gitignore (language-specific)
    write_file(p, ".gitignore", &gitignore_for(language));

    // Language-specific files
    match language {
        "python" => scaffold_python(p),
        "php" => scaffold_php(p),
        "ruby" => scaffold_ruby(p),
        "nodejs" => scaffold_nodejs(p),
        _ => {}
    }

    println!("  {} Scaffold created", "✓".green());
}

fn write_file(base: &Path, rel: &str, content: &str) {
    let full = base.join(rel);
    if let Some(parent) = full.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&full, content).unwrap_or_else(|e| {
        eprintln!(
            "{} Failed to write {}: {}",
            "✗".red(),
            full.display(),
            e
        );
        std::process::exit(1);
    });
}

fn gitignore_for(language: &str) -> String {
    let mut lines = vec![
        ".env",
        "logs/",
        ".DS_Store",
        "*.log",
    ];

    match language {
        "python" => {
            lines.extend_from_slice(&[
                "__pycache__/",
                "*.pyc",
                ".venv/",
                "dist/",
                "*.egg-info/",
            ]);
        }
        "php" => {
            lines.extend_from_slice(&["vendor/", "composer.lock"]);
        }
        "ruby" => {
            lines.extend_from_slice(&[".bundle/", "vendor/bundle/"]);
        }
        "nodejs" => {
            lines.extend_from_slice(&["node_modules/", "dist/", "*.js.map"]);
        }
        _ => {}
    }

    lines.join("\n") + "\n"
}

fn scaffold_python(base: &Path) {
    write_file(
        base,
        "app.py",
        r#"from tina4_python.core import run


if __name__ == "__main__":
    run()
"#,
    );

    write_file(
        base,
        "pyproject.toml",
        r#"[project]
name = "tina4-app"
version = "0.1.0"
description = "A Tina4 Python application"
requires-python = ">=3.10"
dependencies = [
    "tina4-python",
]
"#,
    );
}

fn scaffold_php(base: &Path) {
    write_file(
        base,
        "index.php",
        r#"<?php

require_once __DIR__ . '/vendor/autoload.php';

$app = new \Tina4\App();

$app->start();

$app->dispatch();
"#,
    );

    write_file(
        base,
        "composer.json",
        r#"{
    "name": "tina4/app",
    "description": "A Tina4 PHP application",
    "type": "project",
    "require": {
        "tina4stack/tina4-php": "^3.0"
    },
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}
"#,
    );
}

fn scaffold_ruby(base: &Path) {
    write_file(
        base,
        "app.rb",
        r#"require "tina4"

app = Tina4::App.new
rack = Tina4::RackApp.new(app)
Tina4::WebServer.start(rack)
"#,
    );

    write_file(
        base,
        "Gemfile",
        r#"source "https://rubygems.org"

gem "tina4-ruby", "~> 3.0"
"#,
    );
}

fn scaffold_nodejs(base: &Path) {
    write_file(
        base,
        "app.ts",
        r#"import { startServer } from "tina4-nodejs";

startServer();
"#,
    );

    write_file(
        base,
        "package.json",
        r#"{
    "name": "tina4-app",
    "version": "0.1.0",
    "description": "A Tina4 Node.js application",
    "private": true,
    "scripts": {
        "dev": "npx tsx app.ts",
        "build": "tsc",
        "start": "node dist/app.js"
    },
    "dependencies": {
        "tina4-nodejs": "latest"
    },
    "devDependencies": {
        "tsx": "^4.0.0",
        "typescript": "^5.0.0"
    }
}
"#,
    );

    write_file(
        base,
        "tsconfig.json",
        r#"{
    "compilerOptions": {
        "target": "ES2022",
        "module": "ESNext",
        "moduleResolution": "bundler",
        "esModuleInterop": true,
        "strict": true,
        "outDir": "dist",
        "rootDir": ".",
        "skipLibCheck": true
    },
    "include": ["app.ts", "src/**/*.ts"]
}
"#,
    );
}

// ── Step 4: Install deps ─────────────────────────────────────────

fn install_deps(language: &str, path: &str) {
    println!(
        "\n{} Installing framework dependencies...",
        "▶".green()
    );

    match language {
        "python" => {
            // uv init sets up the venv, then uv add installs tina4-python
            if cmd_exists("uv") {
                run_cmd_in(path, "uv", &["init", "--no-readme"]);
                run_cmd_in(path, "uv", &["add", "tina4-python"]);
            } else {
                eprintln!(
                    "  {} uv not available. Run manually:\n      cd {} && uv init && uv add tina4-python",
                    "⚠".yellow(),
                    path
                );
            }
        }
        "php" => {
            if cmd_exists("composer") {
                run_cmd_in(path, "composer", &["install"]);
            } else {
                eprintln!(
                    "  {} composer not available. Run manually:\n      cd {} && composer install",
                    "⚠".yellow(),
                    path
                );
            }
        }
        "ruby" => {
            if cmd_exists("bundle") {
                run_cmd_in(path, "bundle", &["install"]);
            } else {
                eprintln!(
                    "  {} bundler not available. Run manually:\n      cd {} && bundle install",
                    "⚠".yellow(),
                    path
                );
            }
        }
        "nodejs" => {
            if cmd_exists("npm") {
                run_cmd_in(path, "npm", &["install"]);
            } else {
                eprintln!(
                    "  {} npm not available. Run manually:\n      cd {} && npm install",
                    "⚠".yellow(),
                    path
                );
            }
        }
        _ => {}
    }
}
