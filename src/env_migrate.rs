// Tina4 v3.12 legacy env-var migrator.
//
// v3.12.0 hard-renamed every framework env var with the TINA4_ prefix.
// The framework boot guards refuse to start if they detect any legacy
// un-prefixed name. This module rewrites the user's .env in place,
// renaming legacy keys to their TINA4_* equivalents (preserving values),
// and writes a .env.bak backup first.
//
// Canonical map source: tina4-python/tina4_python/core/server.py
// (`_LEGACY_ENV_VARS`) and tina4-php/Tina4/App.php
// (`App::LEGACY_ENV_VARS`). All four backends share the same 22 entries.

use colored::Colorize;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::console::{icon_info, icon_ok, icon_warn};

/// The 22 v3.12 legacy → canonical rename pairs. Stable; do not reorder
/// without updating the equivalent maps in the four framework backends.
pub fn legacy_env_vars() -> &'static [(&'static str, &'static str)] {
    &[
        ("DATABASE_URL",           "TINA4_DATABASE_URL"),
        ("DATABASE_USERNAME",      "TINA4_DATABASE_USERNAME"),
        ("DATABASE_PASSWORD",      "TINA4_DATABASE_PASSWORD"),
        ("DB_URL",                 "TINA4_DATABASE_URL"),
        ("SECRET",                 "TINA4_SECRET"),
        ("API_KEY",                "TINA4_API_KEY"),
        ("JWT_ALGORITHM",          "TINA4_JWT_ALGORITHM"),
        ("SMTP_HOST",              "TINA4_MAIL_HOST"),
        ("SMTP_PORT",              "TINA4_MAIL_PORT"),
        ("SMTP_USERNAME",          "TINA4_MAIL_USERNAME"),
        ("SMTP_PASSWORD",          "TINA4_MAIL_PASSWORD"),
        ("SMTP_FROM",              "TINA4_MAIL_FROM"),
        ("SMTP_FROM_NAME",         "TINA4_MAIL_FROM_NAME"),
        ("IMAP_HOST",              "TINA4_MAIL_IMAP_HOST"),
        ("IMAP_PORT",              "TINA4_MAIL_IMAP_PORT"),
        ("IMAP_USER",              "TINA4_MAIL_IMAP_USERNAME"),
        ("IMAP_PASS",              "TINA4_MAIL_IMAP_PASSWORD"),
        ("HOST_NAME",              "TINA4_HOST_NAME"),
        ("SWAGGER_TITLE",          "TINA4_SWAGGER_TITLE"),
        ("SWAGGER_DESCRIPTION",    "TINA4_SWAGGER_DESCRIPTION"),
        ("SWAGGER_VERSION",        "TINA4_SWAGGER_VERSION"),
        ("ORM_PLURAL_TABLE_NAMES", "TINA4_ORM_PLURAL_TABLE_NAMES"),
    ]
}

/// Result of a migration plan — what we'd rewrite, what's already
/// canonical, what's untouched. Used both for the dry-run preview and
/// the post-migration summary.
struct Plan {
    /// (legacy_key, new_key, value) — the rewrites.
    rewrites: Vec<(String, String, String)>,
    /// (legacy_key, new_key) — legacy present but new is also already
    /// set, so we drop the legacy without overwriting.
    conflicts: Vec<(String, String)>,
}

impl Plan {
    fn is_empty(&self) -> bool {
        self.rewrites.is_empty() && self.conflicts.is_empty()
    }
}

/// Public entry point — invoked by `tina4 env --migrate`.
pub fn run(yes: bool) {
    println!(
        "\n{}",
        "  Tina4 Env Migration (v3.12 legacy → TINA4_*)  "
            .on_bright_black()
            .white()
    );
    println!();

    let env_path = ".env";
    if !Path::new(env_path).exists() {
        println!(
            "{} No {} file in the current directory — nothing to migrate.",
            icon_info().blue(),
            env_path.cyan()
        );
        println!(
            "  Run `{}` to bootstrap one.",
            "tina4 env --sync".cyan()
        );
        return;
    }

    let raw = match fs::read_to_string(env_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} could not read {}: {}", icon_warn().red(), env_path, e);
            std::process::exit(1);
        }
    };

    let plan = build_plan(&raw);
    if plan.is_empty() {
        println!(
            "{} {} is already TINA4_* clean — no legacy keys found.",
            icon_ok().green(),
            env_path.cyan()
        );
        return;
    }

    print_plan(&plan, env_path);

    if !yes && !confirm() {
        println!("{} aborted; nothing was changed.", icon_info().yellow());
        return;
    }

    // Backup before writing. Existing .env.bak is overwritten — that
    // matches `cp` semantics and keeps the operation idempotent.
    let bak_path = format!("{}.bak", env_path);
    if let Err(e) = fs::write(&bak_path, &raw) {
        eprintln!(
            "{} could not write backup to {}: {}",
            icon_warn().red(),
            bak_path,
            e
        );
        std::process::exit(1);
    }

    let rewritten = apply_plan(&raw, &plan);
    if let Err(e) = fs::write(env_path, rewritten) {
        eprintln!("{} could not write {}: {}", icon_warn().red(), env_path, e);
        std::process::exit(1);
    }

    println!();
    println!(
        "{} migrated {} legacy key{} in {}",
        icon_ok().green(),
        plan.rewrites.len().to_string().cyan(),
        if plan.rewrites.len() == 1 { "" } else { "s" },
        env_path.cyan()
    );
    println!(
        "  backup saved to {} (delete it once you've verified the migration)",
        bak_path.cyan()
    );
}

/// Inspect the .env contents and decide what to rewrite. We work on the
/// raw text rather than a parsed map so we can preserve comments and
/// blank lines (the user's .env is human-edited, not generated).
fn build_plan(raw: &str) -> Plan {
    let map = legacy_env_vars();
    let existing_keys = collect_keys(raw);

    let mut rewrites = Vec::new();
    let mut conflicts = Vec::new();

    for (legacy, new) in map {
        if !existing_keys.contains(&(*legacy).to_string()) {
            continue;
        }
        let value = read_value(raw, legacy).unwrap_or_default();

        if existing_keys.contains(&(*new).to_string()) {
            // Both legacy AND new key exist — assume the user already
            // started migrating manually. Honour their intent: drop the
            // legacy line, leave the canonical value alone.
            conflicts.push(((*legacy).to_string(), (*new).to_string()));
        } else {
            rewrites.push(((*legacy).to_string(), (*new).to_string(), value));
        }
    }

    Plan { rewrites, conflicts }
}

/// Apply the plan to the raw text. For each rewrite, replace the legacy
/// `KEY=value` line with `NEW_KEY=value` in place. Conflicting legacy
/// lines (where the canonical key already exists) are commented out
/// rather than deleted, so users can spot what we did.
fn apply_plan(raw: &str, plan: &Plan) -> String {
    let mut out = String::with_capacity(raw.len() + 256);

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Find the key on this line. Anything before the first '=' that
        // is a valid identifier counts; everything else is left alone.
        let key = line.split('=').next().map(str::trim).unwrap_or("");
        if key.is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Rewrite branch: legacy key → canonical key, same value.
        if let Some((_, new, _)) = plan
            .rewrites
            .iter()
            .find(|(legacy, _, _)| legacy == key)
        {
            // Preserve everything after the '=' verbatim — that includes
            // the user's quoting style, inline comments, and trailing
            // whitespace.
            let after_eq = line.split_once('=').map(|p| p.1).unwrap_or("");
            out.push_str(new);
            out.push('=');
            out.push_str(after_eq);
            out.push('\n');
            continue;
        }

        // Conflict branch: legacy key whose new key is already set —
        // comment out so the user can see we dropped it.
        if plan
            .conflicts
            .iter()
            .any(|(legacy, _)| legacy == key)
        {
            out.push_str("# (tina4 env --migrate dropped this — TINA4_* equivalent already set) ");
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Unrelated line — pass through.
        out.push_str(line);
        out.push('\n');
    }

    // If the original file didn't end with a newline, don't introduce
    // one. The above loop always appends '\n' per line; trim the last
    // one if the source didn't have it.
    if !raw.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    out
}

/// Pull the value associated with `key` from a raw .env body. Returns
/// the substring after the first `=` on the matching line, untrimmed.
fn read_value(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Collect every `KEY` that appears on a non-comment, non-blank line.
/// Used by `build_plan` to detect both legacy presence and canonical
/// presence in a single pass.
fn collect_keys(raw: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((k, _)) = line.split_once('=') {
            keys.push(k.trim().to_string());
        }
    }
    keys
}

fn print_plan(plan: &Plan, env_path: &str) {
    if !plan.rewrites.is_empty() {
        println!(
            "{} renaming {} legacy key{} in {}:",
            icon_info().blue(),
            plan.rewrites.len().to_string().cyan(),
            if plan.rewrites.len() == 1 { "" } else { "s" },
            env_path.cyan()
        );
        for (legacy, new, _) in &plan.rewrites {
            println!("  {} {} → {}", "•".dimmed(), legacy.yellow(), new.green());
        }
        println!();
    }
    if !plan.conflicts.is_empty() {
        println!(
            "{} {} legacy key{} would conflict with an existing TINA4_* key — those will be commented out instead of overwriting:",
            icon_warn().yellow(),
            plan.conflicts.len().to_string().cyan(),
            if plan.conflicts.len() == 1 { "" } else { "s" }
        );
        for (legacy, new) in &plan.conflicts {
            println!(
                "  {} {} (kept {} as-is)",
                "•".dimmed(),
                legacy.yellow(),
                new.cyan()
            );
        }
        println!();
    }
}

fn confirm() -> bool {
    print!(
        "Proceed? A backup will be saved to .env.bak first. [{}/{}] ",
        "y".green(),
        "n".red()
    );
    let _ = io::stdout().flush();
    let mut buf = String::new();
    if io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    matches!(buf.trim().to_lowercase().as_str(), "y" | "yes")
}

// ── tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_map_is_22_entries() {
        // Locked at 22 — must stay in lockstep with the framework-side
        // LEGACY_ENV_VARS maps. If you add an entry here, add it there
        // too (and the matching docs).
        assert_eq!(legacy_env_vars().len(), 22);
    }

    #[test]
    fn build_plan_finds_legacy_keys_only() {
        let raw = "DATABASE_URL=sqlite:///app.db\nTINA4_DEBUG=true\nSECRET=hunter2\n";
        let plan = build_plan(raw);
        assert_eq!(plan.rewrites.len(), 2);
        assert!(plan.rewrites.iter().any(|(l, _, _)| l == "DATABASE_URL"));
        assert!(plan.rewrites.iter().any(|(l, _, _)| l == "SECRET"));
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn build_plan_skips_when_canonical_already_set() {
        let raw = "DATABASE_URL=old\nTINA4_DATABASE_URL=new\n";
        let plan = build_plan(raw);
        assert!(plan.rewrites.is_empty());
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].0, "DATABASE_URL");
    }

    #[test]
    fn apply_plan_rewrites_legacy_keys_and_preserves_values() {
        let raw = "DATABASE_URL=sqlite:///app.db\nTINA4_DEBUG=true\n";
        let plan = build_plan(raw);
        let out = apply_plan(raw, &plan);
        assert!(out.contains("TINA4_DATABASE_URL=sqlite:///app.db"));
        assert!(!out.contains("\nDATABASE_URL="));
        // Untouched lines stay verbatim.
        assert!(out.contains("TINA4_DEBUG=true"));
    }

    #[test]
    fn apply_plan_preserves_comments_and_blank_lines() {
        let raw = "# config\n\nSECRET=hunter2\n# trailing\n";
        let plan = build_plan(raw);
        let out = apply_plan(raw, &plan);
        assert!(out.starts_with("# config\n\nTINA4_SECRET=hunter2\n# trailing\n"));
    }

    #[test]
    fn apply_plan_comments_out_conflicts() {
        let raw = "DATABASE_URL=old\nTINA4_DATABASE_URL=new\n";
        let plan = build_plan(raw);
        let out = apply_plan(raw, &plan);
        // Conflict line gets commented; canonical line untouched.
        assert!(out.contains("# (tina4 env --migrate dropped this — TINA4_* equivalent already set) DATABASE_URL=old"));
        assert!(out.contains("TINA4_DATABASE_URL=new"));
    }

    #[test]
    fn apply_plan_preserves_quoted_values_and_inline_comments() {
        let raw = "SECRET=\"my secret\"  # change in prod\n";
        let plan = build_plan(raw);
        let out = apply_plan(raw, &plan);
        // Whatever sits after the first '=' is preserved verbatim — same
        // quoting, same trailing comment.
        assert!(out.contains("TINA4_SECRET=\"my secret\"  # change in prod"));
    }

    #[test]
    fn apply_plan_no_op_when_no_legacy_keys() {
        let raw = "TINA4_DEBUG=true\nTINA4_SECRET=hunter2\n";
        let plan = build_plan(raw);
        assert!(plan.is_empty());
        let out = apply_plan(raw, &plan);
        assert_eq!(out, raw);
    }

    #[test]
    fn apply_plan_preserves_missing_trailing_newline() {
        // .env files commonly end without a trailing newline (POSIX
        // convention is the opposite, but editors vary). Don't add one
        // if the input didn't have one.
        let raw = "SECRET=hunter2";
        let plan = build_plan(raw);
        let out = apply_plan(raw, &plan);
        assert_eq!(out, "TINA4_SECRET=hunter2");
    }
}
