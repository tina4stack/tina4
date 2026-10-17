// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `tina4 uninstall` — remove everything the client put on this machine:
//! the config + credentials, the installed Tina4 AI skills, and the binary
//! itself. It lists exactly what it will touch first, asks before it removes
//! anything (unless `--yes`), and `--dry-run` only reports. It never hunts the
//! filesystem: per-project state (`./.tina4/`, `.tina4-metrics.json`,
//! `.tina4-docs/`) is only offered for the current directory.

use crate::console::{icon_fail, icon_info, icon_ok, icon_warn, is_windows};
use colored::*;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One thing to remove, with a human label for the plan + confirmation.
struct Target {
    label: String,
    path: PathBuf,
}

pub fn run(dry_run: bool, assume_yes: bool) {
    let global = collect_global();
    let binary = crate::console::current_exe().ok().filter(|p| p.exists());
    let project = collect_project();

    println!();
    println!("  {}", "tina4 uninstall".bold());
    println!("  {}", "─".repeat(60));

    if global.is_empty() && binary.is_none() && project.is_empty() {
        println!("  {} Nothing to remove — tina4 left no state on this machine.", icon_info());
        return;
    }

    print_plan(&global, &binary, &project);

    if dry_run {
        println!("\n  {} --dry-run: nothing was removed.", icon_info());
        return;
    }

    if !assume_yes && !confirm("\n  Remove the global tina4 state listed above? [y/N]: ") {
        println!("  {} Cancelled — nothing removed.", icon_info());
        return;
    }

    let mut removed = 0usize;
    let mut failed = 0usize;
    remove_all(&global, &mut removed, &mut failed);

    // Per-project cleanup is a separate, explicit opt-in.
    if !project.is_empty() && (assume_yes || confirm("  Also remove this project's tina4 files? [y/N]: ")) {
        remove_all(&project, &mut removed, &mut failed);
    }

    // The binary goes last so earlier steps still have a working CLI if run via
    // `tina4 uninstall` (they are self-contained, but it keeps the order sane).
    if let Some(b) = binary {
        remove_binary(&b, &mut removed, &mut failed);
    }

    print_summary(removed, failed);
}

/// Print everything the run will touch, before a single thing is removed, so the
/// maintainer can read the whole footprint and say no.
fn print_plan(global: &[Target], binary: &Option<PathBuf>, project: &[Target]) {
    println!("  The following will be removed:");
    for t in global {
        println!("    {} {:<22} {}", "·".dimmed(), t.label, t.path.display().to_string().dimmed());
    }
    if let Some(b) = binary {
        println!("    {} {:<22} {}", "·".dimmed(), "Binary", b.display().to_string().dimmed());
    }
    if !project.is_empty() {
        println!("  And, for THIS project directory (asked separately):");
        for t in project {
            println!("    {} {:<22} {}", "·".dimmed(), t.label, t.path.display().to_string().dimmed());
        }
    }
}

/// Remove a list of targets, tallying successes and failures. A missing path is
/// a success (`remove_path` is idempotent), so a half-removed install finishes
/// cleanly on a second run.
fn remove_all(targets: &[Target], removed: &mut usize, failed: &mut usize) {
    for t in targets {
        match remove_path(&t.path) {
            Ok(()) => *removed += 1,
            Err(e) => {
                *failed += 1;
                eprintln!("    {} {}: {e}", icon_fail().red(), t.path.display());
            }
        }
    }
}

fn print_summary(removed: usize, failed: usize) {
    println!();
    if failed == 0 {
        println!("  {} tina4 uninstalled — {removed} item(s) removed.", icon_ok().green());
    } else {
        println!(
            "  {} Removed {removed} item(s); {failed} could not be removed (see above).",
            icon_warn().yellow()
        );
        std::process::exit(1);
    }
}

/// Global footprint: the config/credentials dir and the installed Tina4 skills
/// (the `tina4-*` skill folders + the `.tina4-skills-ref` marker in each tool's
/// skills home). Only tina4-owned entries are listed — never a whole skills dir.
fn collect_global() -> Vec<Target> {
    let mut targets = Vec::new();

    if let Some(cfg) = crate::auth::config_dir() {
        if cfg.exists() {
            targets.push(Target { label: "Config + credentials".into(), path: cfg });
        }
    }

    for home in skills_homes() {
        if let Ok(entries) = std::fs::read_dir(&home) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("tina4-") && entry.path().is_dir() {
                    targets.push(Target { label: format!("Skill ({name})"), path: entry.path() });
                }
            }
        }
        let marker = home.join(".tina4-skills-ref");
        if marker.exists() {
            targets.push(Target { label: "Skills ref marker".into(), path: marker });
        }
    }

    targets
}

/// Current-directory Tina4 state only — never a filesystem-wide search.
fn collect_project() -> Vec<Target> {
    [
        (".tina4", "Project state (.tina4)"),
        (".tina4-metrics.json", "Metrics baseline"),
        (".tina4-docs", "Downloaded docs"),
    ]
    .iter()
    .filter_map(|(p, label)| {
        let path = PathBuf::from(p);
        path.exists().then(|| Target { label: (*label).into(), path })
    })
    .collect()
}

/// The three skills homes the installer writes to (Claude / Codex / Cursor),
/// honouring the same `TINA4_SKILLS_HOME` override the installer uses.
fn skills_homes() -> Vec<PathBuf> {
    let base = std::env::var_os("TINA4_SKILLS_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()).map(PathBuf::from));
    match base {
        Some(h) => vec![
            h.join(".claude").join("skills"),
            h.join(".agents").join("skills"),
            h.join(".cursor").join("skills"),
        ],
        None => Vec::new(),
    }
}

fn remove_path(path: &Path) -> Result<(), String> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Remove the running binary. On Unix the inode survives until the process
/// exits, so deleting it now is safe. On Windows a running executable cannot
/// delete itself, so print the exact command instead of failing silently.
fn remove_binary(binary: &Path, removed: &mut usize, failed: &mut usize) {
    if is_windows() {
        println!(
            "  {} The binary can't delete itself on Windows. Remove it manually:\n      del \"{}\"",
            icon_info(),
            binary.display()
        );
        return;
    }
    match std::fs::remove_file(binary) {
        Ok(()) => *removed += 1,
        Err(e) => {
            *failed += 1;
            eprintln!(
                "  {} Could not remove the binary {}: {e}\n      Remove it manually: rm \"{}\"",
                icon_fail().red(),
                binary.display(),
                binary.display()
            );
        }
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => false,
        _ => {
            let a = input.trim().to_lowercase();
            a == "y" || a == "yes"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn tmp() -> PathBuf {
        let n = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("tina4_uninstall_{}_{}", std::process::id(), n));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn remove_path_handles_file_dir_and_missing() {
        let root = tmp();
        // a file
        let f = root.join("a.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(remove_path(&f).is_ok());
        assert!(!f.exists());
        // a dir with contents
        let d = root.join("sub");
        std::fs::create_dir_all(d.join("inner")).unwrap();
        std::fs::write(d.join("inner").join("y"), "y").unwrap();
        assert!(remove_path(&d).is_ok());
        assert!(!d.exists());
        // missing path is a no-op success (idempotent uninstall)
        assert!(remove_path(&root.join("nope")).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn collect_project_lists_only_present_current_dir_entries() {
        // collect_project reads the CWD; just assert it never panics and returns
        // only existing entries (the repo root has none of the three).
        for t in collect_project() {
            assert!(t.path.exists(), "listed a non-existent path: {}", t.path.display());
        }
    }
}
