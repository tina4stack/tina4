use colored::Colorize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::console::{icon_fail, icon_info, icon_ok, icon_play, icon_warn};

/// Known Tina4 environment variables with defaults and descriptions.
fn known_vars() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    // (name, default, description, group)
    vec![
        // Server
        ("TINA4_DEBUG", "true", "Enable debug mode (dev toolbar, error overlay, hot-reload)", "Server"),
        ("TINA4_LOG_LEVEL", "ALL", "Log level: ALL, DEBUG, INFO, WARNING, ERROR", "Server"),
        ("TINA4_PORT", "", "Server port (default: auto-detected by framework)", "Server"),
        ("TINA4_NO_BROWSER", "false", "Don't open browser on startup", "Server"),
        ("TINA4_NO_RELOAD", "false", "Disable hot-reload (useful for AI-assisted development)", "Server"),
        ("TINA4_NO_AI_PORT", "false", "Disable test port (port+1000)", "Server"),

        // Database
        ("DATABASE_URL", "sqlite:///data/app.db", "Database connection string", "Database"),
        ("DATABASE_USERNAME", "", "Database username (if not in URL)", "Database"),
        ("DATABASE_PASSWORD", "", "Database password (if not in URL)", "Database"),
        ("TINA4_AUTOCOMMIT", "false", "Auto-commit database transactions", "Database"),
        ("TINA4_DB_CACHE", "false", "Enable query result caching", "Database"),
        ("TINA4_DB_CACHE_TTL", "300", "Query cache TTL in seconds", "Database"),

        // Auth
        ("TINA4_TOKEN_LIMIT", "60", "JWT token expiry in minutes", "Auth"),
        ("TINA4_TOKEN_EXPIRES_IN", "60", "JWT token expiry (alias)", "Auth"),
        ("TINA4_API_KEY", "", "API key for key-based authentication", "Auth"),

        // Session
        ("TINA4_SESSION_BACKEND", "file", "Session backend: file, redis, valkey, mongodb, database", "Session"),
        ("TINA4_SESSION_TTL", "3600", "Session TTL in seconds", "Session"),
        ("TINA4_SESSION_SAMESITE", "Lax", "Cookie SameSite attribute: Strict, Lax, None", "Session"),
        ("TINA4_SESSION_PATH", "data/sessions", "File session storage path", "Session"),
        ("TINA4_SESSION_REDIS_HOST", "localhost", "Redis host for session storage", "Session"),
        ("TINA4_SESSION_REDIS_PORT", "6379", "Redis port for session storage", "Session"),
        ("TINA4_SESSION_REDIS_PASSWORD", "", "Redis password", "Session"),
        ("TINA4_SESSION_REDIS_DB", "0", "Redis database number", "Session"),

        // CORS
        ("TINA4_CORS_ORIGINS", "*", "Allowed CORS origins (comma-separated or *)", "CORS"),
        ("TINA4_CORS_METHODS", "GET,POST,PUT,PATCH,DELETE,OPTIONS", "Allowed HTTP methods", "CORS"),
        ("TINA4_CORS_HEADERS", "Content-Type,Authorization", "Allowed headers", "CORS"),
        ("TINA4_CORS_CREDENTIALS", "false", "Allow credentials", "CORS"),
        ("TINA4_CORS_MAX_AGE", "86400", "Preflight cache duration (seconds)", "CORS"),

        // Security Headers
        ("TINA4_CSP", "default-src 'self'", "Content Security Policy", "Security"),
        ("TINA4_HSTS", "", "Strict-Transport-Security max-age (empty = disabled)", "Security"),
        ("TINA4_FRAME_OPTIONS", "SAMEORIGIN", "X-Frame-Options header", "Security"),
        ("TINA4_REFERRER_POLICY", "strict-origin-when-cross-origin", "Referrer-Policy header", "Security"),
        ("TINA4_PERMISSIONS_POLICY", "camera=(), microphone=(), geolocation=()", "Permissions-Policy header", "Security"),

        // Cache
        ("TINA4_CACHE_BACKEND", "memory", "Cache backend: memory, redis, file", "Cache"),
        ("TINA4_CACHE_TTL", "60", "Response cache TTL in seconds", "Cache"),
        ("TINA4_CACHE_MAX_ENTRIES", "1000", "Maximum cached entries", "Cache"),
        ("TINA4_CACHE_URL", "", "Cache backend URL (for redis)", "Cache"),
        ("TINA4_CACHE_DIR", "data/cache", "Cache directory (for file backend)", "Cache"),

        // Mail
        ("TINA4_MAIL_HOST", "", "SMTP server host", "Mail"),
        ("TINA4_MAIL_PORT", "587", "SMTP port", "Mail"),
        ("TINA4_MAIL_USERNAME", "", "SMTP username", "Mail"),
        ("TINA4_MAIL_PASSWORD", "", "SMTP password", "Mail"),
        ("TINA4_MAIL_FROM", "", "Default from email address", "Mail"),
        ("TINA4_MAIL_FROM_NAME", "", "Default from name", "Mail"),
        ("TINA4_MAIL_ENCRYPTION", "tls", "Encryption: none, tls, ssl", "Mail"),

        // Queue
        ("TINA4_QUEUE_BACKEND", "file", "Queue backend: file, rabbitmq, kafka, mongodb", "Queue"),
        ("TINA4_QUEUE_PATH", "data/queue", "Queue storage path (file backend)", "Queue"),
        ("TINA4_QUEUE_URL", "", "Queue backend URL", "Queue"),
        ("TINA4_RABBITMQ_HOST", "localhost", "RabbitMQ host", "Queue"),
        ("TINA4_RABBITMQ_PORT", "5672", "RabbitMQ port", "Queue"),
        ("TINA4_RABBITMQ_USERNAME", "guest", "RabbitMQ username", "Queue"),
        ("TINA4_RABBITMQ_PASSWORD", "guest", "RabbitMQ password", "Queue"),

        // Localization
        ("TINA4_LOCALE", "en", "Default locale", "Localization"),
        ("TINA4_LOCALE_DIR", "src/locales", "Locale files directory", "Localization"),

        // WebSocket
        ("TINA4_WS_BACKPLANE", "", "WebSocket backplane: redis or nats", "WebSocket"),
        ("TINA4_WS_BACKPLANE_URL", "", "Backplane connection URL", "WebSocket"),
        ("TINA4_WS_MAX_CONNECTIONS", "1000", "Maximum concurrent WebSocket connections", "WebSocket"),

        // Rate Limiting
        ("TINA4_RATE_LIMIT", "100", "Requests per window", "Rate Limiting"),
        ("TINA4_RATE_WINDOW", "60", "Rate limit window in seconds", "Rate Limiting"),
    ]
}

/// Interactive features with option selection.
struct Feature {
    name: &'static str,
    env_key: &'static str,
    options: Vec<&'static str>,
    default: &'static str,
    follow_up: Vec<(&'static str, &'static str, &'static str)>, // (env_key, prompt, default)
}

fn interactive_features() -> Vec<Feature> {
    vec![
        Feature {
            name: "Database",
            env_key: "DATABASE_URL",
            options: vec!["sqlite", "postgres", "mysql", "mssql", "firebird", "mongodb"],
            default: "sqlite",
            follow_up: vec![],
        },
        Feature {
            name: "Cache Backend",
            env_key: "TINA4_CACHE_BACKEND",
            options: vec!["memory", "redis", "file"],
            default: "memory",
            follow_up: vec![
                ("TINA4_CACHE_URL", "Redis URL", "redis://localhost:6379"),
                ("TINA4_CACHE_TTL", "Cache TTL (seconds)", "60"),
            ],
        },
        Feature {
            name: "Session Backend",
            env_key: "TINA4_SESSION_BACKEND",
            options: vec!["file", "redis", "valkey", "mongodb", "database"],
            default: "file",
            follow_up: vec![
                ("TINA4_SESSION_TTL", "Session TTL (seconds)", "3600"),
            ],
        },
        Feature {
            name: "Queue Backend",
            env_key: "TINA4_QUEUE_BACKEND",
            options: vec!["file", "rabbitmq", "kafka", "mongodb"],
            default: "file",
            follow_up: vec![],
        },
    ]
}

/// Read current .env file into a map.
fn read_env(path: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(contents) = fs::read_to_string(path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
                map.insert(key, value);
            }
        }
    }
    map
}

/// Write .env file preserving existing values, adding missing ones.
fn write_env(path: &str, vars: &BTreeMap<String, String>) {
    let mut contents = String::new();

    // Group by known categories
    let known = known_vars();
    let mut groups: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    let mut used_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (name, _, _, group) in &known {
        if let Some(value) = vars.get(*name) {
            groups.entry(group).or_default().push((name.to_string(), value.clone()));
            used_keys.insert(name.to_string());
        }
    }

    // Write grouped vars
    for (group, entries) in &groups {
        contents.push_str(&format!("# {}\n", group));
        for (key, value) in entries {
            contents.push_str(&format!("{}={}\n", key, value));
        }
        contents.push('\n');
    }

    // Write any custom vars not in known list
    for (key, value) in vars {
        if !used_keys.contains(key) {
            contents.push_str(&format!("{}={}\n", key, value));
        }
    }

    fs::write(path, contents).unwrap_or_else(|e| {
        eprintln!("{} Failed to write {}: {}", icon_fail().red(), path, e);
    });
}

/// Generate .env.example with all known vars, grouped and commented.
fn write_env_example(path: &str) {
    let mut contents = String::new();
    contents.push_str("# ─────────────────────────────────────────\n");
    contents.push_str("# Tina4 Environment Configuration\n");
    contents.push_str("# TINA4 — The Intelligent Native Application 4ramework\n");
    contents.push_str("# Generated by: tina4 env\n");
    contents.push_str("# ─────────────────────────────────────────\n\n");

    let known = known_vars();
    let mut current_group = "";

    for (name, default, desc, group) in &known {
        if *group != current_group {
            if !current_group.is_empty() {
                contents.push('\n');
            }
            contents.push_str(&format!("# {}\n", group));
            current_group = group;
        }

        if default.is_empty() {
            contents.push_str(&format!("# {}=                          # {}\n", name, desc));
        } else {
            contents.push_str(&format!("# {}={}  # {}\n", name, default, desc));
        }
    }

    fs::write(path, contents).unwrap_or_else(|e| {
        eprintln!("{} Failed to write {}: {}", icon_fail().red(), path, e);
    });
}

/// Scan source files for TINA4_* and DATABASE_* env var references.
fn scan_env_vars(root: &str) -> Vec<String> {
    let mut found = std::collections::HashSet::new();
    let extensions = ["py", "php", "rb", "ts", "js", "rs"];

    fn walk(dir: &Path, found: &mut std::collections::HashSet<String>, extensions: &[&str]) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || name == "vendor"
                    || name == "__pycache__" || name == ".venv" || name == "dist"
                    || name == "target" || name == "data" || name == "logs"
                {
                    continue;
                }
                walk(&path, found, extensions);
            } else if let Some(ext) = path.extension() {
                if extensions.contains(&ext.to_string_lossy().as_ref()) {
                    if let Ok(content) = fs::read_to_string(&path) {
                        for word in content.split(|c: char| !c.is_alphanumeric() && c != '_') {
                            if (word.starts_with("TINA4_") || word.starts_with("DATABASE_"))
                                && word.len() > 6
                                && word.chars().all(|c| c.is_uppercase() || c == '_')
                            {
                                found.insert(word.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    walk(Path::new(root), &mut found, &extensions);
    let mut sorted: Vec<String> = found.into_iter().collect();
    sorted.sort();
    sorted
}

/// Prompt user for input with a default value.
fn prompt(question: &str, default: &str) -> String {
    print!("  > {} [{}]: ", question, default);
    std::io::stdout().flush().ok();

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => default.to_string(),
        _ => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                default.to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

/// Prompt user to select from a list of options.
fn prompt_select(question: &str, options: &[&str], default: &str) -> String {
    let opts_str = options.join(", ");
    print!("  > {} ({}) [{}]: ", question, opts_str, default);
    std::io::stdout().flush().ok();

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => default.to_string(),
        _ => {
            let trimmed = input.trim().to_lowercase();
            if trimmed.is_empty() {
                default.to_string()
            } else if options.contains(&trimmed.as_str()) {
                trimmed
            } else {
                eprintln!(
                    "  {} Invalid option '{}'. Using default: {}",
                    icon_warn().yellow(),
                    trimmed,
                    default
                );
                default.to_string()
            }
        }
    }
}

/// Build a database URL from interactive prompts.
fn build_database_url(engine: &str) -> String {
    match engine {
        "sqlite" => {
            let path = prompt("Database file path", "data/app.db");
            format!("sqlite:///{}", path)
        }
        "postgres" | "mysql" | "mssql" | "firebird" => {
            let default_port = match engine {
                "postgres" => "5432",
                "mysql" => "3306",
                "mssql" => "1433",
                "firebird" => "3050",
                _ => "5432",
            };
            let host = prompt("Host", "localhost");
            let port = prompt("Port", default_port);
            let database = prompt("Database name", "myapp");
            let username = prompt("Username", "");
            let password = prompt("Password", "");

            if username.is_empty() {
                format!("{}://{}:{}/{}", engine, host, port, database)
            } else if password.is_empty() {
                format!("{}://{}@{}:{}/{}", engine, username, host, port, database)
            } else {
                format!("{}://{}:{}@{}:{}/{}", engine, username, password, host, port, database)
            }
        }
        "mongodb" => {
            let host = prompt("Host", "localhost");
            let port = prompt("Port", "27017");
            let database = prompt("Database name", "myapp");
            let username = prompt("Username", "");
            let password = prompt("Password", "");

            if username.is_empty() {
                format!("mongodb://{}:{}/{}", host, port, database)
            } else {
                format!("mongodb://{}:{}@{}:{}/{}", username, password, host, port, database)
            }
        }
        _ => "sqlite:///data/app.db".to_string(),
    }
}

/// Main entry point.
pub fn run(sync: bool, example_only: bool, list_only: bool) {
    println!(
        "\n{}",
        "  Tina4 Environment Configuration  ".on_bright_black().white()
    );
    println!();

    // List mode: just show all env vars found in the project
    if list_only {
        let vars = scan_env_vars(".");
        println!(
            "{} Found {} environment variables in project:",
            icon_info().blue(),
            vars.len().to_string().cyan()
        );
        for var in &vars {
            // Find description from known vars
            let desc = known_vars()
                .iter()
                .find(|(name, _, _, _)| name == var)
                .map(|(_, default, desc, _)| format!("{} (default: {})", desc, default))
                .unwrap_or_else(|| "custom variable".to_string());
            println!("  {} {} — {}", icon_ok().green(), var.cyan(), desc.dimmed());
        }
        return;
    }

    // Example mode: just generate .env.example
    if example_only {
        write_env_example(".env.example");
        println!(
            "{} Generated {}",
            icon_ok().green(),
            ".env.example".cyan()
        );
        return;
    }

    // Sync mode: scan code, update .env with missing vars, generate .env.example
    if sync {
        let scanned = scan_env_vars(".");
        let mut env_vars = read_env(".env");
        let mut added = 0;

        for var in &scanned {
            if !env_vars.contains_key(var) {
                // Find default from known vars
                let default = known_vars()
                    .iter()
                    .find(|(name, _, _, _)| name == var)
                    .map(|(_, d, _, _)| d.to_string())
                    .unwrap_or_default();
                env_vars.insert(var.clone(), default);
                added += 1;
            }
        }

        write_env(".env", &env_vars);
        write_env_example(".env.example");

        println!(
            "{} Scanned {} env vars, added {} new to .env",
            icon_ok().green(),
            scanned.len().to_string().cyan(),
            added.to_string().cyan()
        );
        println!(
            "{} Generated {}",
            icon_ok().green(),
            ".env.example".cyan()
        );
        return;
    }

    // Interactive mode
    let mut env_vars = read_env(".env");

    for feature in interactive_features() {
        println!(
            "  {}\n  {}",
            feature.name.bold(),
            "─".repeat(45)
        );

        let current = env_vars
            .get(feature.env_key)
            .cloned()
            .unwrap_or_else(|| feature.default.to_string());

        if feature.env_key == "DATABASE_URL" {
            println!("  Current: {}", current.cyan());

            let engine = prompt_select(
                "Engine",
                &feature.options,
                &current.split("://").next().unwrap_or("sqlite"),
            );

            let url = build_database_url(&engine);
            println!("  {} Set {}={}", icon_ok().green(), feature.env_key.cyan(), url.dimmed());
            env_vars.insert(feature.env_key.to_string(), url.clone());

            // Store username/password separately if provided
            if let Some(at_pos) = url.find('@') {
                let auth_part = &url[url.find("://").unwrap_or(0) + 3..at_pos];
                if let Some((user, pass)) = auth_part.split_once(':') {
                    env_vars.insert("DATABASE_USERNAME".to_string(), user.to_string());
                    env_vars.insert("DATABASE_PASSWORD".to_string(), pass.to_string());
                }
            }
        } else {
            let choice = prompt_select(
                "Choose",
                &feature.options,
                &current,
            );

            println!("  {} Set {}={}", icon_ok().green(), feature.env_key.cyan(), choice.dimmed());
            env_vars.insert(feature.env_key.to_string(), choice.clone());

            // Follow-up prompts for non-default choices
            if choice != feature.default {
                for (key, question, default) in &feature.follow_up {
                    let value = prompt(question, default);
                    println!("  {} Set {}={}", icon_ok().green(), key.cyan(), value.dimmed());
                    env_vars.insert(key.to_string(), value);
                }
            }
        }

        println!();
    }

    // Mail configuration
    println!(
        "  {}\n  {}",
        "Mail".bold(),
        "─".repeat(45)
    );
    let configure_mail = prompt("Configure SMTP? (y/N)", "N");
    if configure_mail.to_lowercase() == "y" {
        let host = prompt("SMTP Host", "");
        if !host.is_empty() {
            env_vars.insert("TINA4_MAIL_HOST".to_string(), host);
            let port = prompt("SMTP Port", "587");
            env_vars.insert("TINA4_MAIL_PORT".to_string(), port);
            let user = prompt("Username", "");
            env_vars.insert("TINA4_MAIL_USERNAME".to_string(), user);
            let pass = prompt("Password", "");
            env_vars.insert("TINA4_MAIL_PASSWORD".to_string(), pass);
            let from = prompt("From address", "");
            env_vars.insert("TINA4_MAIL_FROM".to_string(), from);
            let enc = prompt_select("Encryption", &["none", "tls", "ssl"], "tls");
            env_vars.insert("TINA4_MAIL_ENCRYPTION".to_string(), enc);
            println!("  {} Mail configured", icon_ok().green());
        }
    }
    println!();

    // Write files
    write_env(".env", &env_vars);
    write_env_example(".env.example");

    let var_count = env_vars.len();
    println!(
        "{} Updated {} ({} variables)",
        icon_ok().green(),
        ".env".cyan(),
        var_count.to_string().cyan()
    );
    println!(
        "{} Generated {}",
        icon_ok().green(),
        ".env.example".cyan()
    );
    println!();
}
