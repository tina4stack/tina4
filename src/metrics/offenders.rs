// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Offender ranking, rule ids, remedies and --suggest (feature 141). Split out of the metrics engine (ADR-0002); behaviour unchanged.

use super::*;

// ── Offenders ───────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub(crate) struct Offender {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub severity: String,
    pub score: f64,
    pub detail: String,
    /// Stable, never-recycled rule identifier for the SPECIFIC detector that
    /// fired (feature 141). A `kind` is the family; a `rule_id` names the exact
    /// case inside it so a deterministic remedy can be attached. Retire a
    /// published id with a tombstone, never repoint it (the CWE contract).
    pub rule_id: String,
    /// The deterministic structural refactoring this rule points at. It names
    /// the MOVE (extract, unify, split), never a design pattern - naming a
    /// pattern needs the code's intent, which a metric does not carry.
    pub remedy: String,
}

/// The frozen rule catalogue (feature 141). ONE pure function maps a `kind`
/// (plus, for `complexity`, the function's own line count) to its stable
/// `rule_id` and its remedy. Ranges are reserved by category so new detectors
/// take the next free number without ever renumbering an existing one:
///   M0xx complexity/structure of a function, D0xx duplication,
///   S0xx file-level size/structure, C1xx coverage, U9xx tooling.
///
/// Only cases the engine can discriminate from data it ALREADY computes ship a
/// rule id. `complexity` splits on the function's line count - a long function
/// (`loc` >= LONG_METHOD_LOC) is a long method (extract), a short-but-branchy
/// one is dense logic (guard clauses / dispatch). The finer nesting-specific,
/// switch-specific and parameter-count rules are deliberately NOT assigned yet:
/// they need per-function sub-metrics (nesting depth, arm count, parameter
/// count) the AST walk does not produce today, and a rule id that cannot be
/// told apart from its kind would be a synonym, not a layer. Those land as new
/// M0xx numbers when the sub-metrics do - additive, no repoint.
pub(crate) const LONG_METHOD_LOC: usize = 50;
pub(crate) fn rule_for(kind: &str, loc: usize) -> (&'static str, &'static str) {
    match kind {
        "complexity" if loc >= LONG_METHOD_LOC => (
            "TINA4-M001",
            "long method: extract cohesive steps into their own named methods",
        ),
        "complexity" => (
            "TINA4-M002",
            "dense logic: invert with guard clauses, or replace a fat conditional with a dispatch table or polymorphism",
        ),
        "duplication" => (
            "TINA4-D010",
            "unify the repeated blocks into one shared helper (DRY)",
        ),
        "large_file" => (
            "TINA4-S020",
            "split the file by responsibility into smaller cohesive units",
        ),
        "too_many_functions" => (
            "TINA4-S021",
            "group related functions into cohesive units or a separate module",
        ),
        "low_maintainability" => (
            "TINA4-S022",
            "reduce size and branching together; this usually means a split",
        ),
        "no_test_reference" => (
            "TINA4-C101",
            "add a test that references this file (coverage, not a refactor)",
        ),
        "unparsed" => (
            "TINA4-U901",
            "the engine could not read this file; update the CLI or check the file - it was NOT measured",
        ),
        // An unknown kind gets no id rather than a wrong one. Empty strings
        // serialise as absent-meaning fields and print nothing under --suggest.
        _ => ("", ""),
    }
}

pub(crate) fn severity_rank(sev: &str) -> u8 {
    match sev {
        "error" => 2,
        "warn" => 1,
        _ => 0,
    }
}

/// Build the ranked offender list, mirroring `offenders()` in metrics.py: one
/// rule each for function complexity, large_file, too_many_functions,
/// low_maintainability, untested.
pub(crate) fn build_offenders(files: &[FileMetrics], functions: &[FunctionInfo]) -> Vec<Offender> {
    let mut items: Vec<Offender> = Vec::new();

    // Function-level complexity — EVERY function over the threshold becomes an
    // offender, mirroring the Python master fix (fee4385): offenders() reads the
    // FULL, uncapped, complexity-sorted function list, not a display top-15 slice,
    // so a 16th+ over-threshold function is never silently dropped from the
    // offenders list OR from --fail-on. Display truncation is the CLI's `--top N`
    // (applied after the full set drives the exit code); it is not done here.
    let mut by_cc: Vec<&FunctionInfo> = functions.iter().collect();
    by_cc.sort_by(|a, b| b.complexity.cmp(&a.complexity));
    for fn_info in by_cc.iter() {
        let cc = fn_info.complexity;
        if cc > 10 {
            let (rule_id, remedy) = rule_for("complexity", fn_info.loc);
            items.push(Offender {
                file: fn_info.file.clone(),
                line: fn_info.line,
                kind: "complexity".to_string(),
                severity: if cc > 20 { "error" } else { "warn" }.to_string(),
                score: cc as f64,
                // ASCII only. Every other `detail` here already is, and this one
                // em dash made the payload non-ASCII: a consumer whose runtime
                // tags subprocess output with a minimal locale (LANG=C) then
                // raised on the first string operation. Also the house style.
                detail: format!("{} - cyclomatic complexity {}", fn_info.name, cc),
                rule_id: rule_id.to_string(),
                remedy: remedy.to_string(),
            });
        }
    }

    // File-level rules, over files sorted by maintainability ascending.
    let mut by_mi: Vec<&FileMetrics> = files.iter().collect();
    by_mi.sort_by(|a, b| a.maintainability.partial_cmp(&b.maintainability).unwrap_or(std::cmp::Ordering::Equal));
    for fm in by_mi {
        if fm.loc > 500 {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "large_file".to_string(),
                severity: "warn".to_string(),
                score: fm.loc as f64 / 100.0,
                detail: format!("{} LOC (max 500)", fm.loc),
                rule_id: rule_for("large_file", 0).0.to_string(),
                remedy: rule_for("large_file", 0).1.to_string(),
            });
        }
        if fm.functions > 20 {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "too_many_functions".to_string(),
                severity: "warn".to_string(),
                score: fm.functions as f64 / 4.0,
                detail: format!("{} functions (max 20)", fm.functions),
                rule_id: rule_for("too_many_functions", 0).0.to_string(),
                remedy: rule_for("too_many_functions", 0).1.to_string(),
            });
        }
        // A low file-level MI is only a real maintainability signal when the
        // code is genuinely complex. The MI formula is size-dominated (the
        // 16.2*ln(SLOC) term), so a big-but-SIMPLE file scores near 0 purely
        // for being long: MEASURED, 400 branchless functions over 1200 lines
        // (avg CC 1.0) scored MI 8.4 and was raised as an ERROR. That says
        // nothing `large_file` (loc > 500) doesn't already say, and a genuinely
        // hot function is already caught by the per-function `complexity`
        // offender. So without a complexity gate this rule just re-flags size,
        // at error severity, on files whose functions are all trivial.
        //
        // Gate on avg per-function complexity: a file whose average function is
        // at least moderately branchy (avg CC >= 5) AND has a low MI is hard to
        // maintain in a way the size/hot-function rules miss. Below that the low
        // MI is a size artifact and `large_file` owns it.
        const LOW_MI_MIN_AVG_CC: f64 = 5.0;
        if fm.maintainability < 40.0 && fm.avg_complexity >= LOW_MI_MIN_AVG_CC {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "low_maintainability".to_string(),
                severity: if fm.maintainability < 20.0 { "error" } else { "warn" }.to_string(),
                score: 50.0 - fm.maintainability,
                detail: format!(
                    "maintainability index {:.1} (min 40), avg complexity {:.1}",
                    fm.maintainability, fm.avg_complexity
                ),
                rule_id: rule_for("low_maintainability", 0).0.to_string(),
                remedy: rule_for("low_maintainability", 0).1.to_string(),
            });
        }
        if !fm.has_referencing_test {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "no_test_reference".to_string(),
                severity: "info".to_string(),
                score: fm.loc as f64 / 100.0,
                detail: "no referencing test found (this is not coverage)".to_string(),
                rule_id: rule_for("no_test_reference", 0).0.to_string(),
                remedy: rule_for("no_test_reference", 0).1.to_string(),
            });
        }
    }

    sort_offenders(&mut items);
    items
}

/// Sort by (severity rank, score) descending, stable.
pub(crate) fn sort_offenders(items: &mut [Offender]) {
    items.sort_by(|a, b| {
        severity_rank(&b.severity)
            .cmp(&severity_rank(&a.severity))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
}

/// Turn refused files into offenders so they appear in the ranked list, not
/// only in a separate section someone might not read.
///
/// Severity is `warn`, not `error`, and that is a deliberate compatibility
/// call: a file the engine cannot parse is a TOOLING gap (a grammar older than
/// the source it is fed), not a defect in the code being measured, and
/// promoting it to `error` would newly break every existing `--fail-on error`
/// CI run for reasons outside the author's control. `warn` still trips
/// `--fail-on warn`, and the summary line says it loudly either way.
pub(crate) fn refusal_offenders(refused: &[RefusedFile]) -> Vec<Offender> {
    refused
        .iter()
        .map(|r| Offender {
            file: r.path.clone(),
            line: 1,
            kind: "unparsed".to_string(),
            severity: "warn".to_string(),
            // Bigger unreadable files are a bigger hole in the report.
            score: r.lines as f64 / 100.0,
            detail: format!("NOT MEASURED - {}", r.reason),
            rule_id: rule_for("unparsed", 0).0.to_string(),
            remedy: rule_for("unparsed", 0).1.to_string(),
        })
        .collect()
}

/// Turn clone groups into offenders.
///
/// Kept separate from `build_offenders` because duplication is the only rule
/// that is a property of the SCAN rather than of a single file - it cannot be
/// computed from one file's metrics, and folding it in would have forced every
/// existing caller to supply a whole-project argument it does not have.
///
/// Severity mirrors the existing rules rather than inventing a scale: a
/// duplicate spanning many lines or repeated many times is an `error`, anything
/// else over the reporting floor is a `warn`. `score` is lines * copies, which
/// is the number of duplicated lines the codebase would shed by unifying it -
/// the same "how much does fixing this buy" ranking the other rules use.
pub(crate) fn clone_offenders(groups: &[CloneGroup]) -> Vec<Offender> {
    groups
        .iter()
        .map(|g| {
            let wasted = g.lines * g.copies;
            let where_ = if g.cross_file {
                format!(" across {} files", g.files.len())
            } else {
                String::new()
            };
            Offender {
                file: g.first_file.clone(),
                line: g.first_line,
                kind: "duplication".to_string(),
                // Cross-file duplication is the expensive kind - it is the one
                // that drifts out of sync - so it escalates sooner.
                severity: if wasted >= 60 || (g.cross_file && wasted >= 40) {
                    "error"
                } else {
                    "warn"
                }
                .to_string(),
                score: wasted as f64,
                detail: format!(
                    "{} duplicated lines x {} copies{} - {} lines could be removed",
                    g.lines,
                    g.copies,
                    where_,
                    g.lines * (g.copies - 1)
                ),
                rule_id: rule_for("duplication", 0).0.to_string(),
                remedy: rule_for("duplication", 0).1.to_string(),
            }
        })
        .collect()
}
