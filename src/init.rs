use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Run the full init flow: check runtime, check package manager, scaffold, install deps.
pub fn run(lang: Option<&str>, path: Option<&str>) {
    let lang = match lang {
        Some(l) => l,
        None => {
            print_usage();
            std::process::exit(1);
        }
    };

    let path = match path {
        Some(p) => p,
        None => {
            print_usage();
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
                "✗".red(),
                lang
            );
            println!();
            print_usage();
            std::process::exit(1);
        }
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
        "▶".green(),
        language.cyan(),
        abs.cyan()
    );

    // Step 1 -- Check / install language runtime
    check_runtime(language);

    // Step 2 -- Check / install package manager
    check_package_manager(language);

    // Step 3 -- Create project directory
    create_project_dir(abs);

    // Step 4 -- Delegate to language-specific init (scaffold + install deps)
    delegate_init(language, abs);

    // Step 5 -- Create .env file
    create_env_file(abs);

    // Step 6 -- Summary
    println!();
    println!("Project created at {}", abs);
    println!();
    println!("Next steps:");
    println!("  cd {}", abs);
    println!("  tina4 serve");
    println!();
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

// -- Step 1: Runtime ---------------------------------------------------------

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
                    if !run_cmd("brew", &["install", "python"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Python 3 manually:\n      https://www.python.org/downloads/",
                            "✗".red()
                        );
                        std::process::exit(1);
                    }
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
                    if !run_cmd("brew", &["install", "php"]) {
                        eprintln!(
                            "  {} brew install failed. Please install PHP manually:\n      https://www.php.net/downloads",
                            "✗".red()
                        );
                        std::process::exit(1);
                    }
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
                    if !run_cmd("brew", &["install", "ruby"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Ruby manually:\n      https://www.ruby-lang.org/en/downloads/",
                            "✗".red()
                        );
                        std::process::exit(1);
                    }
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
                    if !run_cmd("brew", &["install", "node"]) {
                        eprintln!(
                            "  {} brew install failed. Please install Node.js manually:\n      https://nodejs.org/",
                            "✗".red()
                        );
                        std::process::exit(1);
                    }
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

// -- Step 2: Package manager -------------------------------------------------

fn check_package_manager(language: &str) {
    println!("\n{} Checking package manager...", "▶".green());

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
                        "  {} Failed to install uv. Install it manually:\n      curl -LsSf https://astral.sh/uv/install.sh | sh",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "php" => {
            if cmd_exists("composer") {
                println!("  {} composer found", "✓".green());
            } else {
                println!("  {} composer not found — attempting install", "⚠".yellow());
                if cmd_exists("brew") {
                    if !run_cmd("brew", &["install", "composer"]) {
                        eprintln!(
                            "  {} Failed to install composer. Install it manually:\n      https://getcomposer.org/download/",
                            "✗".red()
                        );
                        std::process::exit(1);
                    }
                } else {
                    eprintln!(
                        "  {} Please install Composer: https://getcomposer.org/download/",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
            }
        }
        "ruby" => {
            if cmd_exists("bundle") {
                println!("  {} bundler found", "✓".green());
            } else {
                println!("  {} bundler not found — installing via gem", "⚠".yellow());
                if !run_cmd("gem", &["install", "bundler"]) {
                    eprintln!(
                        "  {} Failed to install bundler. Install it manually:\n      gem install bundler",
                        "✗".red()
                    );
                    std::process::exit(1);
                }
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
            "⚠".yellow(),
            path
        );
    } else {
        fs::create_dir_all(p).unwrap_or_else(|e| {
            eprintln!(
                "{} Failed to create directory {}: {}",
                "✗".red(),
                path,
                e
            );
            std::process::exit(1);
        });
        println!("  {} Created directory {}", "✓".green(), path);
    }
}

// -- Step 4: Delegate to language-specific init ------------------------------

fn delegate_init(language: &str, path: &str) {
    println!(
        "\n{} Running {} project init...",
        "▶".green(),
        language.cyan()
    );

    match language {
        "python" => {
            // uv init sets up pyproject.toml and venv, then add tina4-python,
            // then run the framework's own init to scaffold routes/templates
            if !run_cmd_in(path, "uv", &["init"]) {
                eprintln!(
                    "  {} uv init failed. Run manually:\n      cd {} && uv init && uv add tina4-python && uv run tina4python init .",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            if !run_cmd_in(path, "uv", &["add", "tina4-python"]) {
                eprintln!(
                    "  {} uv add failed. Run manually:\n      cd {} && uv add tina4-python",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            // Delegate to the framework CLI to scaffold routes, templates, etc.
            if !run_cmd_in(path, "uv", &["run", "tina4python", "init", "."]) {
                println!(
                    "  {} tina4python init skipped (framework CLI may not be available yet)",
                    "⚠".yellow()
                );
            }
        }
        "php" => {
            if !run_cmd_in(
                path,
                "composer",
                &["init", "--no-interaction", "--name=tina4/my-project"],
            ) {
                eprintln!(
                    "  {} composer init failed. Run manually:\n      cd {} && composer init --no-interaction --name=tina4/my-project",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            if !run_cmd_in(path, "composer", &["require", "tina4stack/tina4-php"]) {
                eprintln!(
                    "  {} composer require failed. Run manually:\n      cd {} && composer require tina4stack/tina4-php",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            // Delegate to the framework CLI to scaffold routes, templates, etc.
            if !run_cmd_in(
                path,
                "php",
                &["vendor/bin/tina4php", "init", "."],
            ) {
                println!(
                    "  {} tina4php init skipped (framework CLI may not be available yet)",
                    "⚠".yellow()
                );
            }
        }
        "ruby" => {
            if !run_cmd_in(path, "bundle", &["init"]) {
                eprintln!(
                    "  {} bundle init failed. Run manually:\n      cd {} && bundle init",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            if !run_cmd_in(path, "bundle", &["add", "tina4-ruby"]) {
                eprintln!(
                    "  {} bundle add failed. Run manually:\n      cd {} && bundle add tina4-ruby",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            // Delegate to the framework CLI to scaffold routes, templates, etc.
            if !run_cmd_in(path, "bundle", &["exec", "tina4ruby", "init", "."]) {
                println!(
                    "  {} tina4ruby init skipped (framework CLI may not be available yet)",
                    "⚠".yellow()
                );
            }
        }
        "nodejs" => {
            if !run_cmd_in(path, "npm", &["init", "-y"]) {
                eprintln!(
                    "  {} npm init failed. Run manually:\n      cd {} && npm init -y",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            if !run_cmd_in(path, "npm", &["install", "tina4-nodejs"]) {
                eprintln!(
                    "  {} npm install failed. Run manually:\n      cd {} && npm install tina4-nodejs",
                    "✗".red(),
                    path
                );
                std::process::exit(1);
            }
            // Delegate to the framework CLI to scaffold routes, templates, etc.
            if !run_cmd_in(path, "npx", &["tina4nodejs", "init", "."]) {
                println!(
                    "  {} tina4nodejs init skipped (framework CLI may not be available yet)",
                    "⚠".yellow()
                );
            }
        }
        _ => {}
    }

    println!("  {} Dependencies installed", "✓".green());
}

// -- Step 5: Create .env file ------------------------------------------------

fn create_env_file(path: &str) {
    let env_path = Path::new(path).join(".env");
    if env_path.exists() {
        println!(
            "  {} .env already exists, skipping",
            "⚠".yellow()
        );
        return;
    }
    fs::write(&env_path, "TINA4_DEBUG=true\nTINA4_LOG_LEVEL=ALL\n").unwrap_or_else(|e| {
        eprintln!(
            "  {} Failed to create .env: {}",
            "✗".red(),
            e
        );
    });
    println!("  {} Created .env", "✓".green());
}
