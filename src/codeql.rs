// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `tina4 codeql` -- run CodeQL locally as a pre-check, matching the repos'
//! GitHub "default setup (extended)" scan (the per-language `security-extended`
//! suite) so you see findings BEFORE CI does. Needs the codeql CLI on PATH.
//!
//! It creates a throwaway database per language, analyses it with the exact
//! suite GitHub runs, and prints findings grouped by rule. The process exit
//! code is the total finding count, so it drops into a pre-push hook:
//!   tina4 codeql || { echo "CodeQL findings -- fix before push"; exit 1; }

use colored::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const PACKS: &str =
    "codeql/javascript-queries codeql/ruby-queries codeql/python-queries codeql/actions-queries";

pub fn run(path: Option<String>, mut languages: Vec<String>, no_actions: bool) -> ! {
    if !codeql_present() {
        eprintln!("{} codeql not found on PATH.", "✗".red());
        eprintln!("  Install it once:  {}", "brew install codeql".cyan());
        eprintln!("  then the packs:   codeql pack download {PACKS}");
        std::process::exit(2);
    }

    let root = match std::fs::canonicalize(path.as_deref().unwrap_or(".")) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("{} no such directory", "✗".red());
            std::process::exit(2);
        }
    };

    // Auto-detect languages when none are given: the project's own language plus
    // the GitHub Actions workflow scan (which flags missing-permissions /
    // unpinned-tag). PHP is deliberately skipped -- CodeQL has no PHP support, so
    // CI uses Semgrep there.
    if languages.is_empty() {
        if let Some(info) = crate::detect::detect_language() {
            match info.language.as_str() {
                "nodejs" | "tina4js" => languages.push("javascript-typescript".into()),
                "python" => languages.push("python".into()),
                "ruby" => languages.push("ruby".into()),
                "php" => eprintln!(
                    "{} PHP has no CodeQL support (CI uses Semgrep); skipping the source scan.",
                    "note:".yellow()
                ),
                _ => {}
            }
        }
    }
    if !no_actions && root.join(".github/workflows").is_dir() && !languages.iter().any(|l| l == "actions") {
        languages.push("actions".into());
    }
    if languages.is_empty() {
        eprintln!("{} no CodeQL language detected here -- pass --language", "✗".red());
        std::process::exit(2);
    }

    let work = std::env::temp_dir().join(format!("tina4-codeql-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);

    println!(
        "CodeQL pre-check: {}  [{}]  (suite: security-extended)",
        root.display().to_string().cyan(),
        languages.join(", ")
    );

    let mut total = 0usize;
    for lang in &languages {
        println!("{}", format!("── {lang} ──────────────────────────────").dimmed());
        let db = work.join(format!("db-{lang}"));

        let created = Command::new("codeql")
            .args(["database", "create"])
            .arg(&db)
            .arg(format!("--language={lang}"))
            .arg(format!("--source-root={}", root.display()))
            .args(["--overwrite", "--quiet"])
            .output();
        if !ok(&created) {
            eprintln!("  {} database create failed", "✗".red());
            print_tail(&created);
            continue;
        }

        let pack = match lang.as_str() {
            "javascript-typescript" | "javascript" | "typescript" => "javascript",
            other => other,
        };
        let suite = format!("codeql/{pack}-queries:codeql-suites/{pack}-security-extended.qls");
        let sarif = work.join(format!("{lang}.sarif"));

        let analysed = Command::new("codeql")
            .args(["database", "analyze"])
            .arg(&db)
            .arg(&suite)
            .arg("--format=sarif-latest")
            .arg(format!("--output={}", sarif.display()))
            .arg("--quiet")
            .output();
        if !ok(&analysed) {
            eprintln!("  {} analyze failed", "✗".red());
            print_tail(&analysed);
            continue;
        }

        let found = report(&sarif);
        if found == 0 {
            println!("  {}", "clean".green());
        }
        total += found;
    }

    let _ = std::fs::remove_dir_all(&work);
    println!("{}", "═".repeat(40).dimmed());
    let summary = format!("CodeQL pre-check total findings: {total}");
    println!("{}", if total == 0 { summary.green() } else { summary.red() });
    // Exit code is the finding count -- non-zero gates a pre-push hook.
    std::process::exit(total.min(i32::MAX as usize) as i32);
}

fn codeql_present() -> bool {
    Command::new("codeql")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ok(out: &std::io::Result<std::process::Output>) -> bool {
    out.as_ref().map(|o| o.status.success()).unwrap_or(false)
}

fn print_tail(out: &std::io::Result<std::process::Output>) {
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stderr).lines().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
            eprintln!("    {line}");
        }
    }
}

/// Parse the SARIF, print findings grouped by rule, return the count.
fn report(sarif: &Path) -> usize {
    let data = match std::fs::read_to_string(sarif) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let v: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let results = v["runs"][0]["results"].as_array().cloned().unwrap_or_default();

    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in &results {
        let rid = r["ruleId"].as_str().unwrap_or("?").to_string();
        let loc = &r["locations"][0]["physicalLocation"];
        let file = loc["artifactLocation"]["uri"].as_str().unwrap_or("?");
        let line = loc["region"]["startLine"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        by.entry(rid).or_default().push(format!("{file}:{line}"));
    }
    for (rid, sites) in &by {
        println!("  {} {}", format!("[{}]", sites.len()).yellow(), rid);
        for s in sites.iter().take(6) {
            println!("        {}", s.dimmed());
        }
        if sites.len() > 6 {
            println!("        ... +{} more", sites.len() - 6);
        }
    }
    results.len()
}
