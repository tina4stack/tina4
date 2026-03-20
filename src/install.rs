use colored::Colorize;
use std::process::Command;

pub fn run(lang: &str) {
    let lang_norm = lang.to_lowercase();

    match lang_norm.as_str() {
        "python" | "py" => install_python(),
        "php" => install_php(),
        "ruby" | "rb" => install_ruby(),
        "nodejs" | "node" | "js" => install_nodejs(),
        "all" => {
            install_python();
            install_php();
            install_ruby();
            install_nodejs();
        }
        _ => {
            eprintln!(
                "{} Unknown language: {}. Use: python, php, ruby, nodejs, all",
                "✗".red(),
                lang
            );
            std::process::exit(1);
        }
    }
}

fn install_python() {
    println!("\n{} Installing Python...", "▶".green());

    if check_exists("python3") || check_exists("python") {
        println!("  {} Python already installed", "✓".green());
    } else {
        run_install_commands(&[
            // macOS
            ("brew", &["install", "python@3.12"]),
            // Linux fallback
            ("sudo", &["apt-get", "install", "-y", "python3.12", "python3.12-venv"]),
        ]);
    }

    // Install uv (Python package manager)
    if check_exists("uv") {
        println!("  {} uv already installed", "✓".green());
    } else {
        println!("  {} Installing uv...", "▶".green());
        let _ = Command::new("sh")
            .args(["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"])
            .status();
    }

    // Install tina4python
    install_tina4_cli("tina4python", "uv", &["tool", "install", "tina4-python"]);
}

fn install_php() {
    println!("\n{} Installing PHP...", "▶".green());

    if check_exists("php") {
        println!("  {} PHP already installed", "✓".green());
    } else {
        run_install_commands(&[
            ("brew", &["install", "php@8.3"]),
            ("sudo", &["apt-get", "install", "-y", "php8.3-cli", "php8.3-mbstring", "php8.3-xml", "php8.3-sqlite3"]),
        ]);
    }

    // Install composer
    if check_exists("composer") {
        println!("  {} Composer already installed", "✓".green());
    } else {
        println!("  {} Installing Composer...", "▶".green());
        let script = r#"
            php -r "copy('https://getcomposer.org/installer', 'composer-setup.php');"
            php composer-setup.php --install-dir=/usr/local/bin --filename=composer
            php -r "unlink('composer-setup.php');"
        "#;
        let _ = Command::new("sh").args(["-c", script]).status();
    }

    println!(
        "  {} Install tina4php: composer global require tina4stack/tina4-php",
        "ℹ".blue()
    );
}

fn install_ruby() {
    println!("\n{} Installing Ruby...", "▶".green());

    if check_exists("ruby") {
        let version = get_version("ruby", "--version");
        // Check if it's system Ruby (2.x) vs modern Ruby (3+/4+)
        if version.starts_with("ruby 2") {
            println!(
                "  {} System Ruby {} detected — installing modern Ruby...",
                "⚠".yellow(),
                version.trim()
            );
            let _ = Command::new("brew")
                .args(["install", "ruby"])
                .status();
        } else {
            println!("  {} Ruby already installed ({})", "✓".green(), version.trim());
        }
    } else {
        run_install_commands(&[
            ("brew", &["install", "ruby"]),
            ("sudo", &["apt-get", "install", "-y", "ruby-full"]),
        ]);
    }

    // Install bundler
    if check_exists("bundle") {
        println!("  {} Bundler already installed", "✓".green());
    } else {
        println!("  {} Installing Bundler...", "▶".green());
        let _ = Command::new("gem")
            .args(["install", "bundler"])
            .status();
    }

    // Install tina4ruby
    install_tina4_cli("tina4ruby", "gem", &["install", "tina4ruby"]);
}

fn install_nodejs() {
    println!("\n{} Installing Node.js...", "▶".green());

    if check_exists("node") {
        println!("  {} Node.js already installed", "✓".green());
    } else {
        run_install_commands(&[
            ("brew", &["install", "node@22"]),
            // Linux: use NodeSource
            ("sh", &["-c", "curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt-get install -y nodejs"]),
        ]);
    }

    // npm comes with node, check it
    if check_exists("npm") {
        println!("  {} npm already installed", "✓".green());
    }

    // Install tina4nodejs
    install_tina4_cli("tina4nodejs", "npm", &["install", "-g", "tina4nodejs"]);
}

// ── Helpers ──────────────────────────────────────────────────────

fn check_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn get_version(cmd: &str, flag: &str) -> String {
    Command::new(cmd)
        .arg(flag)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn run_install_commands(attempts: &[(&str, &[&str])]) {
    for (cmd, args) in attempts {
        if check_exists(cmd) {
            println!("  {} Running: {} {}", "▶".green(), cmd, args.join(" "));
            let status = Command::new(cmd).args(*args).status();
            match status {
                Ok(s) if s.success() => {
                    println!("  {} Installed successfully", "✓".green());
                    return;
                }
                _ => continue,
            }
        }
    }
    eprintln!(
        "  {} Could not install automatically. Please install manually.",
        "✗".red()
    );
}

fn install_tina4_cli(cli_name: &str, pkg_cmd: &str, args: &[&str]) {
    if check_exists(cli_name) {
        println!("  {} {} already installed", "✓".green(), cli_name);
    } else if check_exists(pkg_cmd) {
        println!("  {} Installing {}...", "▶".green(), cli_name);
        let _ = Command::new(pkg_cmd).args(args).status();
    } else {
        println!(
            "  {} Cannot install {} — {} not found",
            "⚠".yellow(),
            cli_name,
            pkg_cmd
        );
    }
}
