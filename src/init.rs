use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::console::{self, icon_fail, icon_ok, icon_play, icon_warn};

/// Run the full init flow: check runtime, check package manager, scaffold, install deps.
pub fn run(lang: Option<&str>, path: Option<&str>) {
    let lang_str: String;

    let lang = match lang {
        Some(l) => {
            // Validate it's actually a language, not a path passed as first arg
            let norm = l.to_lowercase();
            if matches!(norm.as_str(), "python" | "py" | "php" | "ruby" | "rb" | "nodejs" | "node" | "js" | "typescript" | "ts") {
                l
            } else {
                // First arg looks like a path, not a language — prompt for language
                eprintln!(
                    "{} No language specified. Please choose one:\n",
                    icon_warn().yellow()
                );
                lang_str = prompt_language();
                // Treat original 'lang' as the path
                return run(Some(&lang_str), Some(l));
            }
        }
        None => {
            lang_str = prompt_language();
            &lang_str
        }
    };

    let path = match path {
        Some(p) => p,
        None => {
            println!();
            eprintln!(
                "{} Missing project path.\n\nUsage: tina4 init <language> <path>\nExample: tina4 init {} ./my-app",
                icon_fail().red(),
                lang
            );
            std::process::exit(1);
        }
    };

    let lang_norm = lang.to_lowercase();

    match lang_norm.as_str() {
        "python" | "py" => init_project("python", path),
        "php" => init_project("php", path),
        "ruby" | "rb" => init_project("ruby", path),
        "nodejs" | "node" | "js" | "typescript" | "ts" => init_project("nodejs", path),
        _ => {
            eprintln!(
                "{} Unknown language: {}. Use: python, php, ruby, nodejs",
                icon_fail().red(),
                lang
            );
            println!();
            print_usage();
            std::process::exit(1);
        }
    }
}

/// Detect installed runtimes and prompt the user to pick one.
fn prompt_language() -> String {
    use std::io::Write;

    let runtimes = [
        ("python", console::python_cmd()),
        ("php", "php"),
        ("ruby", "ruby"),
        ("nodejs", "node"),
    ];

    let available: Vec<(&str, &str)> = runtimes
        .iter()
        .filter(|(_, cmd)| which::which(cmd).is_ok())
        .copied()
        .collect();

    if available.is_empty() {
        eprintln!(
            "{} No supported language runtimes found. Install one of: Python, PHP, Ruby, Node.js",
            icon_fail().red()
        );
        std::process::exit(1);
    }

    if available.len() == 1 {
        let lang = available[0].0;
        println!(
            "{} Only {} detected — using it",
            icon_ok().green(),
            lang.cyan()
        );
        return lang.to_string();
    }

    println!("  Available languages:\n");
    for (i, (lang, _)) in available.iter().enumerate() {
        println!("    {}. {}", i + 1, lang.cyan());
    }
    println!();

    loop {
        print!("  Select language [1-{}]: ", available.len());
        std::io::stdout().flush().ok();

        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(0) | Err(_) => {
                // EOF or error — non-interactive, exit
                eprintln!("\n{} No language selected (non-interactive mode). Use: tina4 init <language> <path>", icon_fail().red());
                std::process::exit(1);
            }
            _ => {}
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(num) = trimmed.parse::<usize>() {
            if num >= 1 && num <= available.len() {
                return available[num - 1].0.to_string();
            }
        }

        // Also accept language name directly
        let lower = trimmed.to_lowercase();
        if let Some((lang, _)) = available.iter().find(|(l, _)| *l == lower) {
            return lang.to_string();
        }

        println!("  Invalid choice. Try again.");
    }
}

fn print_usage() {
    println!("Usage: tina4 init <language> <path>");
    println!();
    println!("Languages: python, php, ruby, nodejs");
    println!("Example:   tina4 init python ./my-app");
}

fn init_project(language: &str, path: &str) {
    let abs_path = to_absolute(path);
    let abs = abs_path.as_str();

    println!(
        "\n{} Initialising {} project at {}",
        icon_play().green(),
        language.cyan(),
        abs.cyan()
    );

    // Step 1 -- Check / install language runtime
    check_runtime(language);

    // Step 2 -- Check / install package manager
    check_package_manager(language);

    // Step 3 -- Create project directory
    create_project_dir(abs);

    // Step 4 -- Create the full scaffold directly (no delegation)
    scaffold_project(language, abs);

    // Step 5 -- Run package manager install (non-fatal)
    install_deps(language, abs);

    // Step 6 -- Offer to serve immediately
    println!();
    println!("{} Project created at {}", icon_ok().green(), abs);
    println!();

    // Ask user if they want to start serving
    use std::io::Write;
    print!("  Start the server now? [Y/n]: ");
    std::io::stdout().flush().ok();

    let mut input = String::new();
    let should_serve = match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => false, // EOF / non-interactive
        _ => {
            let trimmed = input.trim().to_lowercase();
            trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
        }
    };

    if should_serve {
        println!();
        // Change into project dir and run serve
        std::env::set_current_dir(abs).unwrap_or_else(|e| {
            eprintln!("{} Failed to cd into {}: {}", icon_fail().red(), abs, e);
        });
        crate::handle_serve(None, "0.0.0.0", false, false, false);
    } else {
        println!("  To start later:");
        println!("    cd {}", abs);
        println!("    tina4 serve");
        println!();
    }
}

// -- Helpers -----------------------------------------------------------------

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

fn cmd_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn run_cmd(cmd: &str, args: &[&str]) -> bool {
    println!(
        "  {} Running: {} {}",
        icon_play().green(),
        cmd,
        args.join(" ")
    );
    match Command::new(cmd).args(args).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "  {} Command failed (exit {:?})",
                icon_fail().red(),
                s.code()
            );
            false
        }
        Err(e) => {
            eprintln!("  {} Failed to run {}: {}", icon_fail().red(), cmd, e);
            false
        }
    }
}

fn run_cmd_in(dir: &str, cmd: &str, args: &[&str]) -> bool {
    println!(
        "  {} Running: {} {} (in {})",
        icon_play().green(),
        cmd,
        args.join(" "),
        dir
    );
    match Command::new(cmd).args(args).current_dir(dir).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "  {} Command failed (exit {:?}). You can run it manually:\n      cd {} && {} {}",
                icon_fail().red(),
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
                icon_fail().red(),
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

// -- Step 1: Runtime ---------------------------------------------------------

fn check_runtime(language: &str) {
    println!("\n{} Checking {} runtime...", icon_play().green(), language.cyan());

    match language {
        "python" => {
            let py = console::python_cmd();
            if cmd_exists(py) {
                println!("  {} {} found", icon_ok().green(), py);
            } else {
                println!("  {} python not found — attempting install", icon_warn().yellow());
                if console::is_windows() {
                    eprintln!(
                        "  {} Please install Python 3: https://www.python.org/downloads/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                } else if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "python"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Python 3 manually:\n      https://www.python.org/downloads/",
                            icon_fail().red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install Python 3: https://www.python.org/downloads/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "php" => {
            if cmd_exists("php") {
                println!("  {} php found", icon_ok().green());
            } else {
                println!("  {} php not found — attempting install", icon_warn().yellow());
                if console::is_windows() {
                    eprintln!(
                        "  {} Please install PHP: https://www.php.net/downloads",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                } else if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "php"]) {
                        eprintln!(
                            "  {} brew install failed. Please install PHP manually:\n      https://www.php.net/downloads",
                            icon_fail().red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install PHP: https://www.php.net/downloads",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "ruby" => {
            if cmd_exists("ruby") {
                println!("  {} ruby found", icon_ok().green());
            } else {
                println!("  {} ruby not found — attempting install", icon_warn().yellow());
                if console::is_windows() {
                    eprintln!(
                        "  {} Please install Ruby: https://rubyinstaller.org/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                } else if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "ruby"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Ruby manually:\n      https://www.ruby-lang.org/en/downloads/",
                            icon_fail().red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install Ruby: https://www.ruby-lang.org/en/downloads/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "nodejs" => {
            if cmd_exists("node") {
                println!("  {} node found", icon_ok().green());
            } else {
                println!("  {} node not found — attempting install", icon_warn().yellow());
                if console::is_windows() {
                    eprintln!(
                        "  {} Please install Node.js: https://nodejs.org/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                } else if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "node"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Node.js manually:\n      https://nodejs.org/",
                            icon_fail().red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install Node.js: https://nodejs.org/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        _ => {}
    }
}

// -- Step 2: Package manager -------------------------------------------------

fn check_package_manager(language: &str) {
    println!("\n{} Checking package manager...", icon_play().green());

    match language {
        "python" => {
            if cmd_exists("uv") {
                println!("  {} uv found", icon_ok().green());
            } else {
                println!("  {} uv not found — installing", icon_warn().yellow());
                let ok = if console::is_windows() {
                    run_cmd(
                        "powershell",
                        &["-ExecutionPolicy", "ByPass", "-c", "irm https://astral.sh/uv/install.ps1 | iex"],
                    )
                } else {
                    run_cmd(
                        "sh",
                        &["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"],
                    )
                };
                if !ok {
                    eprintln!(
                        "  {} Failed to install uv. Install it manually:\n      curl -LsSf https://astral.sh/uv/install.sh | sh",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "php" => {
            if cmd_exists("composer") {
                println!("  {} composer found", icon_ok().green());
            } else {
                println!("  {} composer not found — attempting install", icon_warn().yellow());
                if console::is_windows() {
                    eprintln!(
                        "  {} Please install Composer: https://getcomposer.org/Composer-Setup.exe",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                } else if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "composer"]) {
                        eprintln!(
                            "  {} Failed to install composer. Install it manually:\n      https://getcomposer.org/download/",
                            icon_fail().red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install Composer: https://getcomposer.org/download/",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "ruby" => {
            if cmd_exists("bundle") {
                println!("  {} bundler found", icon_ok().green());
            } else {
                println!("  {} bundler not found — installing via gem", icon_warn().yellow());
                if !run_cmd("gem", &["install", "bundler"]) {
                    eprintln!(
                        "  {} Failed to install bundler. Install it manually:\n      gem install bundler",
                        icon_fail().red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "nodejs" => {
            if cmd_exists("npm") {
                println!("  {} npm found", icon_ok().green());
            } else {
                eprintln!(
                    "  {} npm not found — it should come with Node.js. Reinstall Node.",
                    icon_fail().red()
                );
                std::process::exit(1);
            }
        }
        _ => {}
    }
}

// -- Step 3: Create project directory ----------------------------------------

fn create_project_dir(path: &str) {
    let p = Path::new(path);
    if p.exists() {
        println!(
            "  {} Directory already exists: {} — using it",
            icon_warn().yellow(),
            path
        );
    } else {
        fs::create_dir_all(p).unwrap_or_else(|e| {
            eprintln!(
                "{} Failed to create directory {}: {}",
                icon_fail().red(),
                path,
                e
            );
            std::process::exit(1);
        });
        println!("  {} Created directory {}", icon_ok().green(), path);
    }
}

// -- Step 4: Scaffold project (direct file creation) -------------------------

fn scaffold_project(language: &str, path: &str) {
    println!(
        "\n{} Scaffolding {} project...",
        icon_play().green(),
        language.cyan()
    );

    // Common directories shared by all languages
    let common_dirs = [
        "src/routes",
        "src/orm",
        "src/templates",
        "src/public/css",
        "src/public/js",
        "src/public/images",
        "src/scss",
        "migrations",
        "data",
        "logs",
    ];

    for dir in &common_dirs {
        let full = Path::new(path).join(dir);
        fs::create_dir_all(&full).unwrap_or_else(|e| {
            eprintln!("  {} Failed to create {}: {}", icon_fail().red(), dir, e);
        });
    }
    println!("  {} Created directory structure", icon_ok().green());

    // .env
    write_file(path, ".env", "TINA4_DEBUG=true\nTINA4_LOG_LEVEL=ALL\n");

    // Language-specific files
    match language {
        "python" => scaffold_python(path),
        "php" => scaffold_php(path),
        "ruby" => scaffold_ruby(path),
        "nodejs" => scaffold_nodejs(path),
        _ => {}
    }
}

fn scaffold_python(path: &str) {
    let project_name = project_name_from_path(path);

    write_file(
        path,
        "app.py",
        "from tina4_python.core import run\n\nrun()\n",
    );

    write_file(
        path,
        ".gitignore",
        ".venv/\n__pycache__/\n*.pyc\n*.pyo\ndata/\nlogs/\nsecrets/\n.env\n",
    );

    let pyproject = format!(
        r#"[project]
name = "{name}"
version = "0.1.0"
description = "A Tina4 Python project"
requires-python = ">=3.12"
dependencies = [
    "tina4-python>=3.1.0",
]

# Database drivers are optional — install only what you need:
#   uv add psycopg2-binary   # PostgreSQL
#   uv add mysql-connector-python  # MySQL
#   uv add pymssql            # MSSQL
#   uv add firebird-driver    # Firebird
#   uv add pymongo            # MongoDB

[tool.hatch.build.targets.wheel]
packages = ["src"]
"#,
        name = project_name
    );
    write_file(path, "pyproject.toml", &pyproject);

    // src/routes/ is created empty — users add routes via gallery or manually

    println!("  {} Created Python scaffold", icon_ok().green());
}

fn scaffold_php(path: &str) {
    let project_name = project_name_from_path(path);

    write_file(
        path,
        "index.php",
        r#"<?php

require_once __DIR__ . '/vendor/autoload.php';

$app = new \Tina4\App();
$app->run();
"#,
    );

    write_file(
        path,
        ".gitignore",
        "vendor/\ndata/\nlogs/\n.env\n",
    );

    let composer = format!(
        r#"{{
    "name": "tina4/{name}",
    "description": "A Tina4 PHP project",
    "type": "project",
    "require": {{
        "tina4stack/tina4php": "^3.0"
    }},
    "autoload": {{
        "psr-4": {{
            "App\\": "src/"
        }}
    }}
}}
"#,
        name = project_name
    );
    write_file(path, "composer.json", &composer);

    // src/routes/ is created empty — users add routes via gallery or manually

    println!("  {} Created PHP scaffold", icon_ok().green());
}

fn scaffold_ruby(path: &str) {
    write_file(
        path,
        "app.rb",
        r#"require "tina4ruby"

Tina4.run!(__dir__)
"#,
    );

    write_file(
        path,
        ".gitignore",
        ".bundle/\nvendor/\ndata/\nlogs/\n.env\nGemfile.lock\n",
    );

    write_file(
        path,
        "Gemfile",
        r#"source "https://rubygems.org"

gem "tina4ruby", "~> 3.0"
"#,
    );

    // src/routes/ is created empty — users add routes via gallery or manually

    println!("  {} Created Ruby scaffold", icon_ok().green());
}

fn scaffold_nodejs(path: &str) {
    let project_name = project_name_from_path(path);

    write_file(
        path,
        "app.ts",
        r#"import { startServer } from "tina4-nodejs";

startServer();
"#,
    );

    write_file(
        path,
        ".gitignore",
        "node_modules/\ndist/\ndata/\nlogs/\n.env\n",
    );

    let package_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "type": "module",
  "description": "A Tina4 Node.js project",
  "main": "app.ts",
  "scripts": {{
    "start": "npx tsx app.ts",
    "build": "tsc"
  }},
  "dependencies": {{
    "tina4-nodejs": "^3.0.0"
  }},
  "devDependencies": {{
    "typescript": "^5.0.0",
    "tsx": "^4.19.0",
    "@types/node": "^20.0.0"
  }}
}}
"#,
        name = project_name
    );
    write_file(path, "package.json", &package_json);

    write_file(
        path,
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "Node16",
    "moduleResolution": "Node16",
    "lib": ["ES2022"],
    "outDir": "./dist",
    "rootDir": ".",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "resolveJsonModule": true,
    "declaration": true
  },
  "include": ["**/*.ts"],
  "exclude": ["node_modules", "dist"]
}
"#,
    );

    // src/routes/ is created empty — users add routes via gallery or manually

    println!("  {} Created Node.js scaffold", icon_ok().green());
}

// -- Step 5: Install dependencies (non-fatal) --------------------------------

fn install_deps(language: &str, path: &str) {
    println!(
        "\n{} Installing dependencies...",
        icon_play().green()
    );

    let ok = match language {
        "python" => run_cmd_in(path, "uv", &["sync"]),
        "php" => run_cmd_in(path, "composer", &["install"]),
        "ruby" => run_cmd_in(path, "bundle", &["install"]),
        "nodejs" => run_cmd_in(path, "npm", &["install"]),
        _ => true,
    };

    if ok {
        println!("  {} Dependencies installed", icon_ok().green());
    } else {
        println!(
            "  {} Dependency install failed — you can run it manually later",
            icon_warn().yellow()
        );
    }
}

// -- File helpers ------------------------------------------------------------

fn write_file(base: &str, rel_path: &str, contents: &str) {
    let full = Path::new(base).join(rel_path);
    if full.exists() {
        println!(
            "  {} {} already exists, skipping",
            icon_warn().yellow(),
            rel_path
        );
        return;
    }
    // Ensure parent directory exists
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("  {} Failed to create directory for {}: {}", icon_fail().red(), rel_path, e);
        });
    }
    fs::write(&full, contents).unwrap_or_else(|e| {
        eprintln!("  {} Failed to write {}: {}", icon_fail().red(), rel_path, e);
    });
    println!("  {} Created {}", icon_ok().green(), rel_path);
}

fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "tina4-project".to_string())
        .replace(' ', "-")
        .to_lowercase()
}
