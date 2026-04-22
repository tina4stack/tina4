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
            if matches!(norm.as_str(), "python" | "py" | "php" | "ruby" | "rb" | "nodejs" | "node" | "typescript" | "ts" | "js" | "tina4js" | "tina4-js" | "frontend") {
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
        // NB: `js` now routes to the browser-side tina4-js SPA (not nodejs).
        //     Backend Node.js lives under `nodejs`, `node`, `typescript`, `ts`.
        //     This matches developer intuition: `js` = frontend JavaScript.
        "nodejs" | "node" | "typescript" | "ts" => init_project("nodejs", path),
        "js" | "tina4js" | "tina4-js" | "frontend" => init_project("tina4js", path),
        _ => {
            eprintln!(
                "{} Unknown language: {}. Use: python, php, ruby, nodejs, js (tina4-js frontend)",
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
        ("tina4js", "npm"),
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
    println!("Languages: python, php, ruby, nodejs, js (tina4-js frontend)");
    println!("Example:   tina4 init python ./my-app");
    println!("           tina4 init js ./my-frontend   (tina4-js SPA)");
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
    let resolved = console::resolve_cmd(cmd);
    println!(
        "  {} Running: {} {}",
        icon_play().green(),
        cmd,
        args.join(" ")
    );
    match Command::new(&resolved).args(args).status() {
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
    let resolved = console::resolve_cmd(cmd);
    println!(
        "  {} Running: {} {} (in {})",
        icon_play().green(),
        cmd,
        args.join(" "),
        dir
    );
    match Command::new(&resolved).args(args).current_dir(dir).status() {
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
        "nodejs" | "tina4js" => {
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
        "nodejs" | "tina4js" => {
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

    // Directories to create
    let common_dirs: Vec<&str> = if language == "tina4js" {
        vec![
            "src/components",
            "src/routes",
            "src/pages",
            "src/public/css",
        ]
    } else {
        vec![
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
        ]
    };

    for dir in &common_dirs {
        let full = Path::new(path).join(dir);
        fs::create_dir_all(&full).unwrap_or_else(|e| {
            eprintln!("  {} Failed to create {}: {}", icon_fail().red(), dir, e);
        });
    }
    println!("  {} Created directory structure", icon_ok().green());

    // .env (backend projects only)
    if language != "tina4js" {
        write_file(path, ".env", "TINA4_DEBUG=true\nTINA4_LOG_LEVEL=ALL\n");
    }

    // Language-specific files
    match language {
        "python" => scaffold_python(path),
        "php" => scaffold_php(path),
        "ruby" => scaffold_ruby(path),
        "nodejs" => scaffold_nodejs(path),
        "tina4js" => scaffold_tina4js(path),
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

// Local development (fastest — built-in socket server with WebSocket support):
//   tina4 serve
//
// Production behind Apache/nginx (see .htaccess or nginx.conf.example):
//   Apache: mod_rewrite routes all requests through this file
//   nginx:  try_files $uri $uri/ /index.php?$query_string
//
// handle() detects the environment automatically:
//   - CLI (tina4 serve): bootstraps routes, server handles dispatch
//   - Apache/nginx/php-fpm: dispatches the current request and outputs response
$app->handle();
"#,
    );

    write_file(
        path,
        ".env",
        "TINA4_DEBUG=true\nSECRET=change-me-in-production\n",
    );

    write_file(
        path,
        ".gitignore",
        "vendor/\ndata/\nlogs/\ncache/\nsecrets/\n.env\n",
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

    // Apache .htaccess — front controller rewrite
    write_file(
        path,
        ".htaccess",
        r#"DirectoryIndex index.php
RewriteEngine On

# Block sensitive files (.env, .htaccess, secrets/)
<FilesMatch "\.(env|htaccess|htpasswd)$">
    Require all denied
</FilesMatch>

# Uncomment below to force HTTPS (production only)
# RewriteCond %{HTTPS} !=on
# RewriteCond %{REQUEST_URI} !^/.well-known/ [NC]
# RewriteRule ^(.*)$ https://%{HTTP_HOST}%{REQUEST_URI} [L,R=301,NE]

# Serve existing files and directories directly
RewriteCond %{REQUEST_FILENAME} -f [OR]
RewriteCond %{REQUEST_FILENAME} -d
RewriteRule ^ - [L]

# Route everything else through Tina4
RewriteRule ^(.*)$ index.php [QSA,L]

# Pass Authorization header to PHP (required for Bearer tokens)
SetEnvIf Authorization .+ HTTP_AUTHORIZATION=$0
"#,
    );

    // nginx config example
    write_file(
        path,
        "nginx.conf.example",
        r#"# Tina4 PHP — nginx configuration
# Copy to /etc/nginx/sites-available/ and adjust server_name, root, fastcgi_pass.

server {
    listen 80;
    server_name example.com;
    root /var/www/tina4;
    index index.php;

    # Block sensitive files
    location ~ /\.(env|htaccess|htpasswd|git) {
        deny all;
        return 404;
    }
    location ~ ^/(secrets|cache)/ {
        deny all;
        return 404;
    }
    location ~ ^/src/(routes|orm|services|app|templates|scss)/ {
        deny all;
        return 404;
    }

    # Static files from src/public/
    location /src/public/ {
        try_files $uri =404;
    }

    # Front controller
    location / {
        try_files $uri $uri/ /index.php?$query_string;
    }

    # PHP-FPM
    location ~ \.php$ {
        fastcgi_pass unix:/run/php/php-fpm.sock;
        # TCP alternative: fastcgi_pass 127.0.0.1:9000;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include fastcgi_params;
        fastcgi_param HTTP_AUTHORIZATION $http_authorization;
        fastcgi_read_timeout 300;
    }

    gzip on;
    gzip_types text/plain text/css application/json application/javascript text/xml;
}
"#,
    );

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

fn scaffold_tina4js(path: &str) {
    let project_name = project_name_from_path(path);

    // package.json
    let package_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "test": "vitest run"
  }},
  "dependencies": {{
    "tina4js": "^1.0.7"
  }},
  "devDependencies": {{
    "vite": "^5.4.0",
    "typescript": "^5.4.0"
  }}
}}
"#,
        name = project_name
    );
    write_file(path, "package.json", &package_json);

    // tsconfig.json
    write_file(
        path,
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"]
  },
  "include": ["src/**/*.ts"]
}
"#,
    );

    // vite.config.ts
    write_file(
        path,
        "vite.config.ts",
        r#"import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 5173,
    // Proxy API calls to tina4-php/python backend in dev
    // proxy: { '/api': 'http://localhost:7145' },
  },
});
"#,
    );

    // index.html
    let index_html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{name}</title>
  <link rel="stylesheet" href="/src/public/css/default.css">
</head>
<body>
  <div id="root"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
"#,
        name = project_name
    );
    write_file(path, "index.html", &index_html);

    // src/main.ts
    write_file(
        path,
        "src/main.ts",
        r#"import { signal, computed, html, route, router, navigate, api } from 'tina4js';
import './routes/index';

// Debug overlay in dev mode (Ctrl+Shift+D to toggle, tree-shaken from production builds)
if (import.meta.env.DEV) import('tina4js/debug');

// Configure API (uncomment to connect to tina4-php/python backend)
// api.configure({ baseUrl: '/api', auth: true });

// Start router
router.start({ target: '#root', mode: 'hash' });
"#,
    );

    // src/routes/index.ts
    write_file(
        path,
        "src/routes/index.ts",
        r#"import { route, navigate, html, signal, computed } from 'tina4js';
import { homePage } from '../pages/home';

// Home
route('/', homePage);

// About
route('/about', () => html`
  <div class="page">
    <h1>About</h1>
    <p>Built with <a href="https://github.com/tina4stack/tina4-js">tina4-js</a> — a sub-3KB reactive framework.</p>
    <a href="/">Back home</a>
  </div>
`);

// 404
route('*', () => html`
  <div class="page">
    <h1>404</h1>
    <p>Page not found.</p>
    <a href="/">Go home</a>
  </div>
`);
"#,
    );

    // src/pages/home.ts
    write_file(
        path,
        "src/pages/home.ts",
        r#"import { signal, computed, html } from 'tina4js';

export function homePage() {
  const count = signal(0);
  const doubled = computed(() => count.value * 2);

  // Star wiggle animation (matches backend frameworks)
  setTimeout(() => {
    const star = document.querySelector('.star');
    if (star) {
      const wiggle = () => {
        star.classList.add('wiggle');
        setTimeout(() => star.classList.remove('wiggle'), 600);
        setTimeout(wiggle, 3000 + Math.random() * 15000);
      };
      wiggle();
    }
  }, 3000);

  return html`
    <div class="welcome">
      <div class="star">&#9733;</div>
      <h1>Tina4<span class="js">js</span></h1>
      <p class="tagline">The Intelligent Native Application 4ramework</p>
      <p class="version">v${(window as any).TINA4JS_VERSION || '1.0'} &mdash; Sub-3KB Reactive Frontend</p>

      <div class="features">
        <div class="feature">
          <strong>Signals</strong>
          <span>Reactive state</span>
        </div>
        <div class="feature">
          <strong>Components</strong>
          <span>Web Components</span>
        </div>
        <div class="feature">
          <strong>Router</strong>
          <span>SPA navigation</span>
        </div>
        <div class="feature">
          <strong>API</strong>
          <span>HTTP client</span>
        </div>
      </div>

      <div class="counter">
        <button @click=${() => count.value--}>-</button>
        <span>${count}</span>
        <button @click=${() => count.value++}>+</button>
      </div>
      <p class="muted">Doubled: ${doubled}</p>

      <div class="links">
        <a href="https://tina4.com/js" target="_blank">Documentation</a>
        <a href="https://github.com/tina4stack/tina4-js" target="_blank">GitHub</a>
        <a href="/about">About</a>
      </div>

      <p class="hint">Edit <code>src/pages/home.ts</code> to get started. Press <kbd>Ctrl+Shift+D</kbd> for debug overlay.</p>
    </div>
  `;
}
"#,
    );

    // src/components/app-header.ts
    write_file(
        path,
        "src/components/app-header.ts",
        r#"import { Tina4Element, html } from 'tina4js';

class AppHeader extends Tina4Element {
  static props = { title: String };
  static styles = `
    :host { display: block; padding: 1rem 0; border-bottom: 1px solid #e5e7eb; margin-bottom: 2rem; }
    h1 { margin: 0; font-size: 1.5rem; }
    nav { display: flex; gap: 1rem; margin-top: 0.5rem; }
    a { color: #2563eb; text-decoration: none; }
    a:hover { text-decoration: underline; }
  `;

  render() {
    return html`
      <h1>${this.prop('title')}</h1>
      <nav>
        <a href="/">Home</a>
        <a href="/about">About</a>
      </nav>
    `;
  }
}

customElements.define('app-header', AppHeader);
"#,
    );

    // src/public/css/default.css
    write_file(
        path,
        "src/public/css/default.css",
        r#"/* Tina4js — default dark theme (matches backend frameworks) */
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

body {
  font-family: system-ui, -apple-system, sans-serif;
  line-height: 1.6;
  color: #cdd6f4;
  background: #1e1e2e;
  min-height: 100vh;
  display: flex;
  justify-content: center;
  align-items: center;
}

a { color: #89b4fa; text-decoration: none; }
a:hover { text-decoration: underline; }
code { background: #313244; padding: 0.2em 0.5em; border-radius: 4px; font-size: 0.85em; color: #a6e3a1; }
kbd { background: #313244; padding: 0.15em 0.4em; border-radius: 3px; font-size: 0.8em; border: 1px solid #45475a; }

.welcome {
  text-align: center;
  padding: 3rem 2rem;
}

.welcome h1 {
  font-size: 3.5rem;
  font-weight: 800;
  color: #cdd6f4;
  margin-bottom: 0.25rem;
}

.welcome .js {
  color: #f9e2af;
  font-weight: 400;
  font-size: 2rem;
}

.welcome .tagline {
  color: #6c7086;
  font-style: italic;
  margin-bottom: 0.5rem;
}

.welcome .version {
  color: #a6adc8;
  font-size: 0.85rem;
  margin-bottom: 2rem;
}

.star {
  font-size: 4rem;
  color: #f9e2af;
  margin-bottom: 1rem;
  display: inline-block;
  transition: transform 0.3s;
}

.star.wiggle {
  animation: wiggle 0.6s ease-in-out;
}

@keyframes wiggle {
  0%, 100% { transform: rotate(0deg); }
  25% { transform: rotate(-15deg); }
  50% { transform: rotate(15deg); }
  75% { transform: rotate(-10deg); }
}

.features {
  display: flex;
  gap: 1.5rem;
  justify-content: center;
  flex-wrap: wrap;
  margin-bottom: 2rem;
}

.feature {
  background: #313244;
  border-radius: 8px;
  padding: 1rem 1.25rem;
  min-width: 120px;
}

.feature strong {
  display: block;
  color: #cba6f7;
  font-size: 0.9rem;
}

.feature span {
  color: #6c7086;
  font-size: 0.8rem;
}

.counter {
  display: flex;
  align-items: center;
  gap: 1rem;
  justify-content: center;
  margin: 1.5rem 0;
}

.counter button {
  width: 44px;
  height: 44px;
  border: 1px solid #45475a;
  border-radius: 8px;
  background: #313244;
  color: #cdd6f4;
  font-size: 1.25rem;
  cursor: pointer;
  transition: background 0.15s;
}

.counter button:hover { background: #45475a; }

.counter span {
  font-size: 2.5rem;
  font-weight: bold;
  min-width: 3rem;
  color: #a6e3a1;
}

.muted { color: #6c7086; font-size: 0.875rem; margin-bottom: 2rem; }

.links {
  display: flex;
  gap: 1.5rem;
  justify-content: center;
  margin-bottom: 2rem;
}

.links a {
  background: #313244;
  padding: 0.5rem 1rem;
  border-radius: 6px;
  color: #89b4fa;
  font-size: 0.9rem;
  transition: background 0.15s;
}

.links a:hover { background: #45475a; text-decoration: none; }

.hint {
  color: #585b70;
  font-size: 0.8rem;
}

.page { padding: 2rem; max-width: 800px; margin: 0 auto; }
nav { margin: 1.5rem 0; display: flex; gap: 1rem; justify-content: center; }
"#,
    );

    // .gitignore
    write_file(
        path,
        ".gitignore",
        "node_modules/\ndist/\n*.tsbuildinfo\n",
    );

    // .env (not needed for tina4js but keep consistent — skip the common one)

    println!("  {} Created tina4-js scaffold", icon_ok().green());
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
        "nodejs" | "tina4js" => run_cmd_in(path, "npm", &["install"]),
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
