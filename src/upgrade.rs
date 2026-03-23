use crate::console::{icon_fail, icon_info, icon_ok, icon_play, icon_warn};
use colored::Colorize;
use std::fs;
use std::path::Path;

/// Run the v2 → v3 upgrade.
pub fn run() {
    println!(
        "\n{}",
        "  Tina4 Upgrade — v2 → v3  ".on_bright_black().white()
    );
    println!();

    let lang = detect_v2_project();
    if lang.is_none() {
        eprintln!(
            "{} No Tina4 v2 project detected in current directory",
            icon_fail().red()
        );
        eprintln!(
            "{} This command upgrades v2 projects. If this is already v3, no action needed.",
            icon_info().blue()
        );
        std::process::exit(1);
    }

    let lang = lang.unwrap();
    println!(
        "{} Detected v2 {} project — upgrading to v3",
        icon_play().green(),
        lang.cyan()
    );

    let mut changes = 0;

    // Step 1: Directory restructure — move top-level dirs into src/
    changes += move_dir_into_src("routes");
    changes += move_dir_into_src("orm");
    changes += move_dir_into_src("templates");
    changes += move_dir_into_src("scss");
    changes += move_dir_into_src("public");
    changes += move_dir_into_src("services");
    changes += move_dir_into_src("app");
    changes += move_dir_into_src("locales");
    changes += move_dir_into_src("seeds");

    // Ensure src/ exists even if nothing was moved
    if !Path::new("src").exists() {
        fs::create_dir_all("src").ok();
    }

    // Step 2: Update dependency versions in manifest files
    changes += upgrade_manifest(&lang);

    // Step 3: Delegate language-specific code migrations to the language CLI
    // (if the language CLI has an upgrade command)
    delegate_upgrade(&lang);

    println!();
    if changes > 0 {
        println!(
            "{} Upgrade complete — {} changes applied",
            icon_ok().green(),
            changes.to_string().cyan()
        );
    } else {
        println!(
            "{} Project already appears to be v3 structure — no changes needed",
            icon_info().blue()
        );
    }

    println!(
        "{} Review the changes and run your test suite to verify",
        icon_info().blue()
    );
    println!();
}

/// Detect a v2 project by looking for top-level routes/orm dirs (v3 has them under src/).
fn detect_v2_project() -> Option<String> {
    // v2 indicator: routes/ or orm/ at top level (not inside src/)
    let has_toplevel_routes = Path::new("routes").is_dir() && !Path::new("src/routes").is_dir();
    let has_toplevel_orm = Path::new("orm").is_dir() && !Path::new("src/orm").is_dir();
    let has_toplevel_templates =
        Path::new("templates").is_dir() && !Path::new("src/templates").is_dir();

    if !has_toplevel_routes && !has_toplevel_orm && !has_toplevel_templates {
        return None;
    }

    // Detect language
    if Path::new("composer.json").exists() {
        if let Ok(content) = fs::read_to_string("composer.json") {
            if content.contains("tina4") {
                return Some("php".into());
            }
        }
    }
    if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() {
        return Some("python".into());
    }
    if Path::new("Gemfile").exists() {
        return Some("ruby".into());
    }
    if Path::new("package.json").exists() {
        return Some("nodejs".into());
    }
    // Could be any language — still has v2 structure
    Some("unknown".into())
}

/// Move a top-level directory into src/ if it exists at the top level.
fn move_dir_into_src(dir_name: &str) -> usize {
    let src = Path::new(dir_name);
    let dest = Path::new("src").join(dir_name);

    if !src.is_dir() {
        return 0;
    }
    if dest.exists() {
        println!(
            "  {} {} — src/{} already exists, skipping",
            icon_warn().yellow(),
            dir_name,
            dir_name
        );
        return 0;
    }

    // Ensure src/ exists
    fs::create_dir_all("src").ok();

    match fs::rename(src, &dest) {
        Ok(_) => {
            println!(
                "  {} Moved {}/ → src/{}/",
                icon_ok().green(),
                dir_name,
                dir_name
            );
            1
        }
        Err(e) => {
            // rename fails across filesystems; fall back to copy + remove
            if copy_dir_recursive(src, &dest).is_ok() {
                fs::remove_dir_all(src).ok();
                println!(
                    "  {} Moved {}/ → src/{}/",
                    icon_ok().green(),
                    dir_name,
                    dir_name
                );
                1
            } else {
                eprintln!(
                    "  {} Failed to move {}/: {}",
                    icon_fail().red(),
                    dir_name,
                    e
                );
                0
            }
        }
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Update dependency versions in manifest files to v3.
fn upgrade_manifest(lang: &str) -> usize {
    match lang {
        "php" => upgrade_composer_json(),
        "python" => upgrade_pyproject_toml(),
        "ruby" => upgrade_gemfile(),
        "nodejs" => upgrade_package_json(),
        _ => 0,
    }
}

fn upgrade_composer_json() -> usize {
    let path = "composer.json";
    if let Ok(content) = fs::read_to_string(path) {
        // Update tina4php dependency from v2 to v3
        let updated = content
            .replace("\"tina4stack/tina4php\": \"^2", "\"tina4stack/tina4php\": \"^3")
            .replace("\"tina4stack/tina4php\": \"~2", "\"tina4stack/tina4php\": \"^3")
            // Remove old split packages (v2 had separate packages)
            .replace("\"tina4stack/tina4php-core\"", "\"_removed_tina4php-core\"")
            .replace("\"tina4stack/tina4php-database\"", "\"_removed_tina4php-database\"")
            .replace("\"tina4stack/tina4php-orm\"", "\"_removed_tina4php-orm\"");
        if updated != content {
            if fs::write(path, &updated).is_ok() {
                println!(
                    "  {} Updated composer.json — tina4php ^3.0",
                    icon_ok().green()
                );
                if updated.contains("_removed_") {
                    println!(
                        "  {} Removed old split packages (tina4php-core, -database, -orm) — v3 is unified",
                        icon_info().blue()
                    );
                }
                return 1;
            }
        }
    }
    0
}

fn upgrade_pyproject_toml() -> usize {
    let path = "pyproject.toml";
    if let Ok(content) = fs::read_to_string(path) {
        let updated = content
            .replace("tina4-python>=2", "tina4-python>=3")
            .replace("tina4-python~=2", "tina4-python>=3")
            .replace("tina4-python==2", "tina4-python>=3");
        if updated != content {
            if fs::write(path, &updated).is_ok() {
                println!(
                    "  {} Updated pyproject.toml — tina4-python >=3",
                    icon_ok().green()
                );
                return 1;
            }
        }
    }
    // Also check requirements.txt
    let req_path = "requirements.txt";
    if let Ok(content) = fs::read_to_string(req_path) {
        let updated = content
            .replace("tina4-python>=2", "tina4-python>=3")
            .replace("tina4-python~=2", "tina4-python>=3")
            .replace("tina4-python==2", "tina4-python>=3");
        if updated != content {
            if fs::write(req_path, &updated).is_ok() {
                println!(
                    "  {} Updated requirements.txt — tina4-python >=3",
                    icon_ok().green()
                );
                return 1;
            }
        }
    }
    0
}

fn upgrade_gemfile() -> usize {
    let path = "Gemfile";
    if let Ok(content) = fs::read_to_string(path) {
        let updated = content
            .replace("'tina4', '~> 2", "'tina4', '~> 3")
            .replace("\"tina4\", \"~> 2", "\"tina4\", \"~> 3");
        if updated != content {
            if fs::write(path, &updated).is_ok() {
                println!(
                    "  {} Updated Gemfile — tina4 ~> 3.0",
                    icon_ok().green()
                );
                return 1;
            }
        }
    }
    0
}

fn upgrade_package_json() -> usize {
    let path = "package.json";
    if let Ok(content) = fs::read_to_string(path) {
        let updated = content
            .replace("\"@tina4/core\": \"^2", "\"@tina4/core\": \"^3")
            .replace("\"@tina4/core\": \"~2", "\"@tina4/core\": \"^3")
            .replace("\"@tina4/orm\": \"^2", "\"@tina4/orm\": \"^3")
            .replace("\"@tina4/orm\": \"~2", "\"@tina4/orm\": \"^3");
        if updated != content {
            if fs::write(path, &updated).is_ok() {
                println!(
                    "  {} Updated package.json — @tina4/* ^3.0",
                    icon_ok().green()
                );
                return 1;
            }
        }
    }
    0
}

/// Delegate language-specific code upgrades to the language CLI if available.
fn delegate_upgrade(lang: &str) {
    let (cmd, cli_path) = match lang {
        "php" => {
            let vendor_path = crate::console::php_vendor_bin("tina4php");
            if Path::new(&vendor_path).exists() {
                ("php".to_string(), vendor_path)
            } else {
                return;
            }
        }
        "python" => {
            if which::which("tina4python").is_ok() {
                ("tina4python".to_string(), String::new())
            } else {
                return;
            }
        }
        "ruby" => {
            if which::which("tina4ruby").is_ok() {
                ("tina4ruby".to_string(), String::new())
            } else {
                return;
            }
        }
        "nodejs" => {
            if which::which("tina4nodejs").is_ok() {
                ("tina4nodejs".to_string(), String::new())
            } else {
                return;
            }
        }
        _ => return,
    };

    println!(
        "\n  {} Running language-specific upgrade via {} ...",
        icon_play().green(),
        cmd.cyan()
    );

    let status = if lang == "php" {
        std::process::Command::new(&cmd)
            .args([cli_path.as_str(), "upgrade"])
            .status()
    } else {
        std::process::Command::new(&cmd)
            .args(["upgrade"])
            .status()
    };

    match status {
        Ok(s) if s.success() => {
            println!(
                "  {} Language-specific upgrade complete",
                icon_ok().green()
            );
        }
        _ => {
            println!(
                "  {} Language CLI upgrade not available — structural migration done",
                icon_info().blue()
            );
        }
    }
}
