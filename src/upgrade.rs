// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
    if let Ok(content) = fs::read_to_string("composer.json") {
        if content.contains("tina4") {
            return Some("php".into());
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
    let Ok(content) = fs::read_to_string(path) else { return 0 };
    let updated = content
        .replace("\"tina4stack/tina4php\": \"^2", "\"tina4stack/tina4php\": \"^3")
        .replace("\"tina4stack/tina4php\": \"~2", "\"tina4stack/tina4php\": \"^3")
        .replace("\"tina4stack/tina4php-core\"", "\"_removed_tina4php-core\"")
        .replace("\"tina4stack/tina4php-database\"", "\"_removed_tina4php-database\"")
        .replace("\"tina4stack/tina4php-orm\"", "\"_removed_tina4php-orm\"");
    if updated != content && fs::write(path, &updated).is_ok() {
        println!("  {} Updated composer.json — tina4php ^3.0", icon_ok().green());
        if updated.contains("_removed_") {
            println!(
                "  {} Removed old split packages (tina4php-core, -database, -orm) — v3 is unified",
                icon_info().blue()
            );
        }
        return 1;
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
        if updated != content && fs::write(path, &updated).is_ok() {
            println!("  {} Updated pyproject.toml — tina4-python >=3", icon_ok().green());
            return 1;
        }
    }
    let req_path = "requirements.txt";
    if let Ok(content) = fs::read_to_string(req_path) {
        let updated = content
            .replace("tina4-python>=2", "tina4-python>=3")
            .replace("tina4-python~=2", "tina4-python>=3")
            .replace("tina4-python==2", "tina4-python>=3");
        if updated != content && fs::write(req_path, &updated).is_ok() {
            println!("  {} Updated requirements.txt — tina4-python >=3", icon_ok().green());
            return 1;
        }
    }
    0
}

fn upgrade_gemfile() -> usize {
    let path = "Gemfile";
    let Ok(content) = fs::read_to_string(path) else { return 0 };
    let updated = content
        .replace("'tina4', '~> 2", "'tina4', '~> 3")
        .replace("\"tina4\", \"~> 2", "\"tina4\", \"~> 3");
    let mut changes = 0;
    if updated != content && fs::write(path, &updated).is_ok() {
        println!("  {} Updated Gemfile — tina4 ~> 3.0", icon_ok().green());
        changes += 1;
    }
    // tina4ruby v3 does not bundle the SQLite driver (ADR-0067); the app declares it.
    if crate::init::ensure_gemfile_sqlite3(Path::new(path)) {
        println!("  {} Added gem \"sqlite3\" to Gemfile — the SQLite driver is an app dependency", icon_ok().green());
        changes += 1;
    }
    changes
}

fn upgrade_package_json() -> usize {
    let path = "package.json";
    let Ok(content) = fs::read_to_string(path) else { return 0 };
    let updated = migrate_nodejs_package_json(&content);
    if updated != content && fs::write(path, &updated).is_ok() {
        println!("  {} Updated package.json — tina4-nodejs ^3.0", icon_ok().green());
        return 1;
    }
    0
}

/// Rewrite a v2 tina4-nodejs `package.json` dependency set to v3.
///
/// In v2 the package was the scoped `@tina4/core` (with `@tina4/orm` as a
/// separate dependency); in v3 it is the single unscoped `tina4-nodejs` package,
/// and the ORM ships as its `tina4-nodejs/orm` subpath export, NOT a separate
/// dependency. The `@tina4` npm scope is no longer ours, so bumping
/// `@tina4/core` to `^3` would resolve to someone else's package — the name has
/// to change, not just the version. Any `@tina4/*` line collapses into one
/// `tina4-nodejs` dependency. Returns the input unchanged when there is nothing
/// tina4-scoped to migrate.
fn migrate_nodejs_package_json(content: &str) -> String {
    if !content.contains("\"@tina4/") {
        return content.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut wrote_tina4 = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("\"@tina4/core\"") || trimmed.starts_with("\"@tina4/orm\"") {
            if !wrote_tina4 {
                let indent = &line[..line.len() - trimmed.len()];
                let comma = if line.trim_end().ends_with(',') { "," } else { "" };
                out.push(format!("{indent}\"tina4-nodejs\": \"^3.0.0\"{comma}"));
                wrote_tina4 = true;
            }
            // Any further @tina4/* line is folded into the single tina4-nodejs dep.
            continue;
        }
        out.push(line.to_string());
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    strip_trailing_json_commas(&result)
}

/// Remove a comma that a dropped last entry may have left before a closing `}`
/// or `]`, so the rewritten JSON stays valid. Char-safe (never splits a
/// multibyte character) and only touches a comma followed solely by whitespace
/// and then a closer.
fn strip_trailing_json_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1; // drop the comma, keep the whitespace that follows
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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

#[cfg(test)]
mod tests {
    use super::{migrate_nodejs_package_json, strip_trailing_json_commas};

    #[test]
    fn migrate_renames_scoped_core_to_unscoped_package() {
        // The @tina4 scope is no longer ours: a v2 @tina4/core must be renamed to
        // the real tina4-nodejs package, never bumped to a squatted @tina4/core@3.
        let v2 = "{\n  \"dependencies\": {\n    \"@tina4/core\": \"^2.5.0\"\n  }\n}\n";
        let out = migrate_nodejs_package_json(v2);
        assert!(out.contains("\"tina4-nodejs\": \"^3.0.0\""), "got:\n{out}");
        assert!(!out.contains("@tina4/core"), "scoped name must be gone:\n{out}");
    }

    #[test]
    fn migrate_folds_core_and_orm_into_one_dependency() {
        // @tina4/orm folds into tina4-nodejs (its /orm subpath), so both scoped
        // deps collapse to a single valid line with no trailing comma.
        let v2 = "{\n  \"dependencies\": {\n    \"@tina4/core\": \"~2.9.1\",\n    \"@tina4/orm\": \"^2.0.0\"\n  }\n}\n";
        let out = migrate_nodejs_package_json(v2);
        assert_eq!(out.matches("tina4-nodejs").count(), 1, "one collapsed dep expected:\n{out}");
        assert!(!out.contains("@tina4/orm"), "orm dep must be dropped:\n{out}");
        // Valid JSON: no dangling comma before the closing brace.
        assert!(!out.contains(",\n  }"), "trailing comma left behind:\n{out}");
    }

    #[test]
    fn migrate_leaves_a_non_tina4_manifest_untouched() {
        let other = "{\n  \"dependencies\": {\n    \"express\": \"^4.0.0\"\n  }\n}\n";
        assert_eq!(migrate_nodejs_package_json(other), other);
    }

    #[test]
    fn strip_trailing_json_commas_only_touches_a_comma_before_a_closer() {
        assert_eq!(strip_trailing_json_commas("[1, 2,\n]"), "[1, 2\n]");
        assert_eq!(strip_trailing_json_commas("{\"a\":1}"), "{\"a\":1}");
        // A comma between values is preserved.
        assert_eq!(strip_trailing_json_commas("[1, 2]"), "[1, 2]");
    }
}
