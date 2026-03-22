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

    // Step 4 -- Create the full scaffold directly (no delegation)
    scaffold_project(language, abs);

    // Step 5 -- Run package manager install (non-fatal)
    install_deps(language, abs);

    // Step 6 -- Summary
    println!();
    println!("{} Project created at {}", "✓".green(), abs);
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

// -- Step 4: Scaffold project (direct file creation) -------------------------

fn scaffold_project(language: &str, path: &str) {
    println!(
        "\n{} Scaffolding {} project...",
        "▶".green(),
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
            eprintln!("  {} Failed to create {}: {}", "✗".red(), dir, e);
        });
    }
    println!("  {} Created directory structure", "✓".green());

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
    "tina4-python>=3.0.0rc1",
]

[tool.hatch.build.targets.wheel]
packages = ["src"]
"#,
        name = project_name
    );
    write_file(path, "pyproject.toml", &pyproject);

    // Sample route
    write_file(
        path,
        "src/routes/hello.py",
        r#"from tina4_python.core.router import get

@get("/hello")
async def hello(request, response):
    """A sample route that returns a greeting."""
    return response({"message": "Hello from Tina4!", "status": "ok"})
"#,
    );

    println!("  {} Created Python scaffold", "✓".green());
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

    // Sample route
    write_file(
        path,
        "src/routes/hello.php",
        r#"<?php

\Tina4\Get::add("/hello", function (\Tina4\Response $response) {
    return $response("Hello from Tina4!", HTTP_OK);
});
"#,
    );

    println!("  {} Created PHP scaffold", "✓".green());
}

fn scaffold_ruby(path: &str) {
    write_file(
        path,
        "app.rb",
        r#"require "tina4"

Tina4.initialize!(__dir__)
Tina4::App.new.run!
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

gem "tina4-ruby", "~> 3.0"
"#,
    );

    // Sample route
    write_file(
        path,
        "src/routes/hello.rb",
        r#"Tina4.get "/hello" do |_request, response|
  response.text "Hello from Tina4!", 200
end
"#,
    );

    println!("  {} Created Ruby scaffold", "✓".green());
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
  "description": "A Tina4 Node.js project",
  "main": "app.ts",
  "scripts": {{
    "start": "ts-node app.ts",
    "build": "tsc"
  }},
  "dependencies": {{
    "tina4-nodejs": "^3.0.0"
  }},
  "devDependencies": {{
    "typescript": "^5.0.0",
    "ts-node": "^10.0.0",
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
    "target": "ES2020",
    "module": "commonjs",
    "lib": ["ES2020"],
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

    // Sample route
    write_file(
        path,
        "src/routes/hello.ts",
        r#"import { get } from "tina4-nodejs";

get("/hello", async (_request, response) => {
    return response("Hello from Tina4!", 200);
});
"#,
    );

    println!("  {} Created Node.js scaffold", "✓".green());
}

// -- Step 5: Install dependencies (non-fatal) --------------------------------

fn install_deps(language: &str, path: &str) {
    println!(
        "\n{} Installing dependencies...",
        "▶".green()
    );

    let ok = match language {
        "python" => run_cmd_in(path, "uv", &["sync"]),
        "php" => run_cmd_in(path, "composer", &["install"]),
        "ruby" => run_cmd_in(path, "bundle", &["install"]),
        "nodejs" => run_cmd_in(path, "npm", &["install"]),
        _ => true,
    };

    if ok {
        println!("  {} Dependencies installed", "✓".green());
    } else {
        println!(
            "  {} Dependency install failed — you can run it manually later",
            "⚠".yellow()
        );
    }
}

// -- File helpers ------------------------------------------------------------

fn write_file(base: &str, rel_path: &str, contents: &str) {
    let full = Path::new(base).join(rel_path);
    if full.exists() {
        println!(
            "  {} {} already exists, skipping",
            "⚠".yellow(),
            rel_path
        );
        return;
    }
    // Ensure parent directory exists
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("  {} Failed to create directory for {}: {}", "✗".red(), rel_path, e);
        });
    }
    fs::write(&full, contents).unwrap_or_else(|e| {
        eprintln!("  {} Failed to write {}: {}", "✗".red(), rel_path, e);
    });
    println!("  {} Created {}", "✓".green(), rel_path);
}

fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "tina4-project".to_string())
        .replace(' ', "-")
        .to_lowercase()
}
