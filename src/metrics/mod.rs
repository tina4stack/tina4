// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native, language-agnostic code-metrics engine (ADR-0002).
//
// Scans SOURCE directly — per file LOC, cyclomatic complexity (McCabe),
// maintainability index (Radon/Microsoft), efferent coupling, function count —
// for Python / PHP / Ruby / TypeScript+JS / Rust, with NO Tina4 project and NO
// running framework required. Replaces the four per-framework metrics modules
// and, for the first time, covers the frontend (tina4-js .ts) and arbitrary
// non-framework code.
//
// Rust was added last and closed a real blind spot: until then the engine could
// not measure its OWN implementation language, so `tina4 metrics` pointed at
// this repo found zero files and the CLI was the one Tina4 codebase nobody could
// audit.
//
// Pascal / Delphi is deliberately ABSENT. The only published grammar crate
// cannot parse Delphi 10.3+ inline loop variables, which puts 51.5% of the real
// tina4delphi corpus inside a parse-error region; see `pascal_is_not_claimed`.
//
// tina4: ADR-0002 — formulas + thresholds originated in the retired Python
// engine. The scope-neutral formula calibration below preserves that baseline;
// callable allocation is locked independently across all five languages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

mod lang;
use lang::{Lang, count_loc, is_code_line, maintainability_index, round_dp};


// ── Per-file result ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FunctionInfo {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub complexity: u32,
    /// Code lines spanned by the function body. The dev dashboard's
    /// "most complex functions" table has a LOC column, so omitting this
    /// rendered a literal `undefined` in every row.
    pub loc: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FileMetrics {
    pub path: String,
    pub loc: usize,
    pub complexity: u32,      // file_complexity: sum of every function's CC
    pub avg_complexity: f64,  // rounded to 2 dp
    pub functions: usize,
    pub maintainability: f64, // rounded to 1 dp, clamped [0, 100]
    // The Halstead volume that maintainability was derived FROM. Already
    // computed for the MI; serialising it costs nothing and closes the gap
    // left when coupling_afferent/instability were added without it.
    pub halstead_volume: f64, // rounded to 2 dp
    pub has_referencing_test: bool,

    // ── Coupling ─────────────────────────────────────────────────────────────
    // `dep_count` is EVERY import the file writes, stdlib and third-party
    // included. It is the number the dev dashboard badges on a bubble, so its
    // meaning is deliberately unchanged.
    //
    // The coupling TRIPLE below is INTERNAL-only: it counts edges between files
    // inside the scanned tree, because that is the only thing an architectural
    // coupling number can mean. Efferent = files I depend on. Afferent = files
    // that depend on me. Instability = ce / (ca + ce), the Martin metric: 0.0 is
    // maximally stable (everyone depends on me, I depend on nobody), 1.0 is
    // maximally unstable (I depend on others, nobody depends on me).
    //
    // Mixing the two is what made the previous implementation meaningless: it
    // divided a total-import efferent count (up to 119) by an afferent count
    // that was always 0, so instability was the constant 1.0 everywhere.
    pub dep_count: usize,
    pub coupling_efferent: usize,
    pub coupling_afferent: usize,
    pub instability: f64, // rounded to 3 dp

    /// Fraction of lines that parsed cleanly (1.0 = perfect). Serialised so a
    /// consumer can see WHY a file was refused, and so a file that is close to
    /// the floor is visible before it drops below it.
    pub parse_health: f64, // rounded to 3 dp
}


mod halstead;
use halstead::*;

mod complexity;
use complexity::*;

mod imports;
use imports::*;

mod analyze;
use analyze::*;

mod clones;
use clones::*;

mod discovery;
use discovery::*;

mod tests_index;
use tests_index::*;

mod offenders;
use offenders::*;

mod report;
use report::*;

// ── CLI entry point ─────────────────────────────────────────────────────────

/// `--fail-on` gate: exit 1 when an offender at/above the requested severity
/// exists. `warn` trips on warn OR error; `error` trips only on error.
fn compute_exit_code(fail_on: Option<&str>, has_warn: bool, has_error: bool) -> i32 {
    match fail_on {
        Some("warn") if has_warn || has_error => 1,
        Some("error") if has_error => 1,
        _ => 0,
    }
}

/// `--fail-on-regression` gate: the concrete reasons this scan is WORSE than the
/// committed baseline, or empty if it is not. Deliberately reuses the changelog's
/// own `regressed_files` classification (a new file carrying an offender, more
/// offenders on a file, or a worse worst-case complexity on a file that still
/// offends) so the gate can never fail on movement the "Since last run" report
/// prints as clean or improved. Adds one summary axis the per-file pass does not
/// cover: a rise in duplicated lines, so the de-duplication win is ratcheted too.
/// Each returned string is a human-readable line naming exactly what regressed.
fn regression_reasons(delta: &RunDelta) -> Vec<String> {
    let mut reasons = Vec::new();
    for f in &delta.regressed_files {
        if f.status == "new" {
            reasons.push(format!(
                "{}  NEW file with {} offender(s) (worst CC {})",
                f.path, f.offenders_after, f.worst_cc_after
            ));
        } else {
            reasons.push(format!(
                "{}  offenders {}->{}, worst CC {}->{}",
                f.path, f.offenders_before, f.offenders_after, f.worst_cc_before, f.worst_cc_after
            ));
        }
    }
    // Duplication is a cross-file summary metric: the per-file pass classifies a
    // duplication change as "changed", never "regressed", so gate it here.
    if delta.after.duplicate_lines > delta.before.duplicate_lines {
        reasons.push(format!(
            "duplicated lines {}->{} (+{})",
            delta.before.duplicate_lines,
            delta.after.duplicate_lines,
            delta.after.duplicate_lines - delta.before.duplicate_lines
        ));
    }
    reasons
}

// ── Run history: regression / improvement tracking ──────────────────────────
// Every scan records a summary plus full per-file snapshots to
// `.tina4-metrics.json` in the scan root, so the NEXT run of the same scope can
// say what improved and what got worse. Zero new dependencies: the record is serde_json and timestamps are
// UNIX seconds from SystemTime. `--no-history` reads and writes nothing.
//
// The file is data, not source (a `.json`), so the engine never scans it and it
// never pollutes its own numbers. Commit it to gate regressions in CI, or add it
// to .gitignore for a purely local baseline - both work.

const HISTORY_FILE: &str = ".tina4-metrics.json";
const HISTORY_SCHEMA: u32 = 2;
const TREND_CAP: usize = 20;

#[derive(Serialize, Deserialize, Clone)]
struct MetricsSnapshot {
    /// UNIX seconds when this scan ran.
    at: u64,
    tool_version: String,
    files_analyzed: usize,
    total_functions: usize,
    avg_complexity: f64,
    avg_maintainability: f64,
    total_offenders: usize,
    duplicate_blocks: usize,
    duplicate_lines: usize,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
struct HistoryFile {
    offenders: usize,
    /// Highest cyclomatic complexity of any function in the file.
    worst_cc: u32,
    maintainability: f64,
    loc: usize,
    complexity: u32,
    avg_complexity: f64,
    functions: usize,
    halstead_volume: f64,
    has_referencing_test: bool,
    dep_count: usize,
    coupling_efferent: usize,
    coupling_afferent: usize,
    instability: f64,
    parse_health: f64,
    duplicate_groups: usize,
    duplicate_lines: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct HistoryRun {
    snapshot: MetricsSnapshot,
    files: BTreeMap<String, HistoryFile>,
}

#[derive(Serialize, Deserialize)]
struct HistoryRecord {
    schema: u32,
    scan_root: String,
    last: MetricsSnapshot,
    files: BTreeMap<String, HistoryFile>,
    /// Older summaries, oldest first, excluding `last`. Capped at TREND_CAP so
    /// the file cannot grow without bound.
    #[serde(default)]
    trend: Vec<MetricsSnapshot>,
    /// Complete per-file snapshots for each retained run, oldest first. This
    /// is the commit-friendly changelog; `last` and `files` remain as a compact
    /// current-run mirror for consumers of schema 1.
    #[serde(default)]
    runs: Vec<HistoryRun>,
}

/// One file's movement between two runs.
#[derive(Serialize)]
struct FileDelta {
    path: String,
    offenders_before: i64,
    offenders_after: i64,
    worst_cc_before: i64,
    worst_cc_after: i64,
    /// Full metric snapshots for this file, so a changelog consumer can see
    /// what changed without reopening the history file and joining paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics_before: Option<HistoryFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics_after: Option<HistoryFile>,
    /// "improved" | "regressed" | "new" | "resolved".
    status: String,
}

/// What changed since the previous recorded run.
#[derive(Serialize)]
struct RunDelta {
    /// Age of the previous run in seconds (how long since the baseline).
    since_secs: u64,
    before: MetricsSnapshot,
    after: MetricsSnapshot,
    improved_files: Vec<FileDelta>,
    regressed_files: Vec<FileDelta>,
    /// Files whose metrics changed without crossing the offender/complexity
    /// improvement gates. This makes clean-file churn visible as well.
    changed_files: Vec<FileDelta>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn history_path(scan_root: &str) -> PathBuf {
    Path::new(scan_root).join(HISTORY_FILE)
}

fn load_history(scan_root: &str) -> Option<HistoryRecord> {
    let raw = fs::read_to_string(history_path(scan_root)).ok()?;
    let rec: HistoryRecord = serde_json::from_str(&raw).ok()?;
    // A record from an incompatible schema is ignored (treated as no baseline)
    // rather than crashing a scan; the next save rewrites it at the current schema.
    if rec.schema != HISTORY_SCHEMA {
        return None;
    }
    Some(rec)
}

fn snapshot_of(summary: &Summary) -> MetricsSnapshot {
    MetricsSnapshot {
        at: now_secs(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        files_analyzed: summary.files_analyzed,
        total_functions: summary.total_functions,
        avg_complexity: summary.avg_complexity,
        avg_maintainability: summary.avg_maintainability,
        total_offenders: summary.total_offenders,
        duplicate_blocks: summary.duplicate_blocks,
        duplicate_lines: summary.duplicate_lines,
    }
}

/// Full per-file metric snapshot for this run, plus offender and duplication
/// signals used by the next run's changelog diff.
fn per_file_history(report: &Report) -> BTreeMap<String, HistoryFile> {
    let mut map: BTreeMap<String, HistoryFile> = BTreeMap::new();
    for f in &report.files {
        map.insert(
            f.path.clone(),
            HistoryFile {
                offenders: 0,
                worst_cc: 0,
                maintainability: f.maintainability,
                loc: f.loc,
                complexity: f.complexity,
                avg_complexity: f.avg_complexity,
                functions: f.functions,
                halstead_volume: f.halstead_volume,
                has_referencing_test: f.has_referencing_test,
                dep_count: f.dep_count,
                coupling_efferent: f.coupling_efferent,
                coupling_afferent: f.coupling_afferent,
                instability: f.instability,
                parse_health: f.parse_health,
                duplicate_groups: 0,
                duplicate_lines: 0,
            },
        );
    }
    for func in &report.functions {
        if let Some(e) = map.get_mut(&func.file) {
            if func.complexity > e.worst_cc {
                e.worst_cc = func.complexity;
            }
        }
    }
    for o in &report.offenders {
        map.entry(o.file.clone()).or_default().offenders += 1;
    }
    for clone in &report.clones {
        for occurrence in &clone.occurrences {
            if let Some(file) = map.get_mut(&occurrence.file) {
                file.duplicate_groups += 1;
                file.duplicate_lines += clone.lines;
            }
        }
    }
    map
}

fn history_file_changed(before: &HistoryFile, after: &HistoryFile) -> bool {
    before != after
}

fn compute_delta(
    prev: &HistoryRecord,
    after: &MetricsSnapshot,
    cur_files: &BTreeMap<String, HistoryFile>,
) -> RunDelta {
    let mut improved: Vec<FileDelta> = Vec::new();
    let mut regressed: Vec<FileDelta> = Vec::new();
    let mut changed: Vec<FileDelta> = Vec::new();

    for (path, cf) in cur_files {
        let before = prev.files.get(path);
        let (ob, wb) = before
            .map(|b| (b.offenders as i64, b.worst_cc as i64))
            .unwrap_or((0, 0));
        let (oa, wa) = (cf.offenders as i64, cf.worst_cc as i64);
        if before.is_none() {
            // A file that did not exist last run. Only worth calling out if it
            // arrives carrying offenders.
            if oa > 0 {
                regressed.push(FileDelta {
                    path: path.clone(),
                    offenders_before: 0,
                    offenders_after: oa,
                    worst_cc_before: 0,
                    worst_cc_after: wa,
                    metrics_before: None,
                    metrics_after: Some(cf.clone()),
                    status: "new".to_string(),
                });
            } else {
                changed.push(FileDelta {
                    path: path.clone(),
                    offenders_before: 0,
                    offenders_after: 0,
                    worst_cc_before: 0,
                    worst_cc_after: wa,
                    metrics_before: None,
                    metrics_after: Some(cf.clone()),
                    status: "new".to_string(),
                });
            }
            continue;
        }
        // Worst-complexity movement only counts when the file already carries an
        // offender: a cc wobble on an otherwise-clean file is summary noise, not a
        // per-file regression, and 0->0 offender lines just clutter the report.
        let cc_moves = oa == ob && ob > 0;
        if oa < ob || (cc_moves && wa < wb) {
            improved.push(FileDelta {
                path: path.clone(),
                offenders_before: ob,
                offenders_after: oa,
                worst_cc_before: wb,
                worst_cc_after: wa,
                metrics_before: before.cloned(),
                metrics_after: Some(cf.clone()),
                status: "improved".to_string(),
            });
        } else if oa > ob || (cc_moves && wa > wb) {
            regressed.push(FileDelta {
                path: path.clone(),
                offenders_before: ob,
                offenders_after: oa,
                worst_cc_before: wb,
                worst_cc_after: wa,
                metrics_before: before.cloned(),
                metrics_after: Some(cf.clone()),
                status: "regressed".to_string(),
            });
        } else if history_file_changed(before.unwrap(), cf) {
            changed.push(FileDelta {
                path: path.clone(),
                offenders_before: ob,
                offenders_after: oa,
                worst_cc_before: wb,
                worst_cc_after: wa,
                metrics_before: before.cloned(),
                metrics_after: Some(cf.clone()),
                status: "changed".to_string(),
            });
        }
    }
    // A file that had offenders last run and is gone now (deleted, or renamed)
    // counts as resolved - its offenders left the report.
    for (path, bf) in &prev.files {
        if !cur_files.contains_key(path) && bf.offenders > 0 {
            improved.push(FileDelta {
                path: path.clone(),
                offenders_before: bf.offenders as i64,
                offenders_after: 0,
                worst_cc_before: bf.worst_cc as i64,
                worst_cc_after: 0,
                metrics_before: Some(bf.clone()),
                metrics_after: None,
                status: "resolved".to_string(),
            });
        } else if !cur_files.contains_key(path) {
            changed.push(FileDelta {
                path: path.clone(),
                offenders_before: 0,
                offenders_after: 0,
                worst_cc_before: bf.worst_cc as i64,
                worst_cc_after: 0,
                metrics_before: Some(bf.clone()),
                metrics_after: None,
                status: "removed".to_string(),
            });
        }
    }

    // Biggest offender swing first.
    improved.sort_by(|a, b| {
        (b.offenders_before - b.offenders_after).cmp(&(a.offenders_before - a.offenders_after))
    });
    regressed.sort_by(|a, b| {
        (b.offenders_after - b.offenders_before).cmp(&(a.offenders_after - a.offenders_before))
    });
    changed.sort_by(|a, b| a.path.cmp(&b.path));

    RunDelta {
        since_secs: now_secs().saturating_sub(prev.last.at),
        before: prev.last.clone(),
        after: after.clone(),
        improved_files: improved,
        regressed_files: regressed,
        changed_files: changed,
    }
}

fn save_history(
    scan_root: &str,
    prev: Option<HistoryRecord>,
    last: MetricsSnapshot,
    files: BTreeMap<String, HistoryFile>,
) {
    let mut trend: Vec<MetricsSnapshot> = Vec::new();
    if let Some(p) = prev.as_ref() {
        trend = p.trend.clone();
        trend.push(p.last.clone()); // the previous run's summary joins the trend line
        let overflow = trend.len().saturating_sub(TREND_CAP);
        if overflow > 0 {
            trend.drain(0..overflow);
        }
    }
    let mut runs = prev
        .as_ref()
        .map(|p| p.runs.clone())
        .unwrap_or_default();
    // A schema-2 record always has runs, but seed the sequence from the current
    // mirror fields if a hand-edited/early record omitted them.
    if runs.is_empty() {
        if let Some(p) = prev.as_ref() {
            runs.push(HistoryRun { snapshot: p.last.clone(), files: p.files.clone() });
        }
    }
    runs.push(HistoryRun { snapshot: last.clone(), files: files.clone() });
    let overflow = runs.len().saturating_sub(TREND_CAP);
    if overflow > 0 {
        runs.drain(0..overflow);
    }
    let rec = HistoryRecord {
        schema: HISTORY_SCHEMA,
        scan_root: scan_root.to_string(),
        last,
        files,
        trend,
        runs,
    };
    if let Ok(json) = serde_json::to_string_pretty(&rec) {
        // Best-effort: a metrics scan must never fail because it could not write
        // its own bookkeeping (read-only dir, CI sandbox). The numbers still print.
        let _ = fs::write(history_path(scan_root), json);
    }
}

/// Human-readable "since last run" block. Direction-aware: fewer offenders,
/// fewer duplicate lines and lower complexity are better; higher maintainability
/// is better.
fn print_delta_human(delta: &RunDelta, scan_root: &str) {
    use std::io::IsTerminal;
    let use_color = std::io::stdout().is_terminal();
    let paint = |text: String, code: &str| -> String {
        if use_color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text
        }
    };
    // GREEN when the movement is an improvement, RED when a regression, dim when flat.
    let tag = |better: bool, worse: bool| -> String {
        if better {
            paint("better".to_string(), "1;32")
        } else if worse {
            paint("worse".to_string(), "1;31")
        } else {
            paint("same".to_string(), "2")
        }
    };
    let b = &delta.before;
    let a = &delta.after;

    println!();
    println!(
        "  Since last run ({}, {})",
        human_age(delta.since_secs),
        b.tool_version
    );
    // offenders: down is better
    let d_off = a.total_offenders as i64 - b.total_offenders as i64;
    println!(
        "    offenders           {:>6} -> {:<6} ({:+})  {}",
        b.total_offenders,
        a.total_offenders,
        d_off,
        tag(d_off < 0, d_off > 0)
    );
    // duplicate lines: down is better
    let d_dup = a.duplicate_lines as i64 - b.duplicate_lines as i64;
    println!(
        "    duplicate lines     {:>6} -> {:<6} ({:+})  {}",
        b.duplicate_lines,
        a.duplicate_lines,
        d_dup,
        tag(d_dup < 0, d_dup > 0)
    );
    // avg maintainability: UP is better
    let d_mnt = round_dp(a.avg_maintainability - b.avg_maintainability, 1);
    println!(
        "    avg maintainability {:>6.1} -> {:<6.1} ({:+.1})  {}",
        b.avg_maintainability,
        a.avg_maintainability,
        d_mnt,
        tag(d_mnt > 0.0, d_mnt < 0.0)
    );
    // avg complexity: down is better
    let d_cpx = round_dp(a.avg_complexity - b.avg_complexity, 2);
    println!(
        "    avg complexity      {:>6.2} -> {:<6.2} ({:+.2})  {}",
        b.avg_complexity,
        a.avg_complexity,
        d_cpx,
        tag(d_cpx < 0.0, d_cpx > 0.0)
    );

    let show = |label: &str, code: &str, list: &[FileDelta]| {
        if list.is_empty() {
            return;
        }
        let head: Vec<String> = list
            .iter()
            .take(5)
            .map(|f| {
                // Show the signal that actually moved: offender count if it
                // changed, otherwise the worst-function complexity.
                let change = if f.offenders_before != f.offenders_after {
                    format!("{}->{}", f.offenders_before, f.offenders_after)
                } else {
                    format!("cc {}->{}", f.worst_cc_before, f.worst_cc_after)
                };
                format!("{} ({})", f.path, change)
            })
            .collect();
        let more = list.len().saturating_sub(5);
        let suffix = if more > 0 {
            format!(", +{more} more")
        } else {
            String::new()
        };
        println!("    {}: {}{}", paint(label.to_string(), code), head.join(", "), suffix);
    };
    show("improved", "1;32", &delta.improved_files);
    show("regressed", "1;31", &delta.regressed_files);
    show("changed", "2", &delta.changed_files);
    let _ = scan_root;
}

/// "just now" / "5m ago" / "3h ago" / "2d ago" - a calendar-free age so the
/// tool needs no date library.
fn human_age(secs: u64) -> String {
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

/// Everything `tina4 metrics` needs to run one scan. Grouped into a struct so
/// the entry point stays a single argument as flags accrue (each new flag would
/// otherwise push `run` past clippy's argument-count limit).
pub struct RunOptions {
    pub path: Option<String>,
    pub top: Option<usize>,
    pub json: bool,
    pub fail_on: Option<String>,
    pub exclusions: Vec<String>,
    pub include_non_production: bool,
    pub no_history: bool,
    pub fail_on_regression: bool,
    /// Print each offender's rule id and remedy under its row (feature 141).
    /// Opt-in and additive: the default human table is unchanged, and the
    /// rule id / remedy are always present in `--json` regardless of this flag.
    pub suggest: bool,
}

/// `tina4 metrics` — native, language-agnostic. Returns the process exit code.
pub fn run(opts: RunOptions) -> i32 {
    let RunOptions {
        path,
        top,
        json,
        fail_on,
        exclusions,
        include_non_production,
        no_history,
        fail_on_regression,
        suggest,
    } = opts;
    if let Some(f) = &fail_on {
        if f != "warn" && f != "error" {
            eprintln!("  invalid --fail-on '{f}' (use warn or error)");
            return 2;
        }
    }
    if fail_on_regression && no_history {
        eprintln!("  --fail-on-regression needs the baseline, so it cannot be combined with --no-history");
        return 2;
    }
    let top = top.unwrap_or(20);

    let (files, scan_root) = match resolve_targets(
        path.as_deref(),
        &exclusions,
        include_non_production,
    ) {
        Ok(v) => v,
        Err(e) => {
            if json {
                println!("{{\n  \"summary\": {{\n    \"error\": {}\n  }},\n  \"offenders\": []\n}}", serde_json::to_string(&e).unwrap_or_default());
            } else {
                println!("  metrics error: {e}");
            }
            return 2;
        }
    };

    let report = analyze_targets(&files, &scan_root);
    let total_offenders = report.offenders.len();
    let summary = build_summary(&report, total_offenders);

    // Run history: snapshot this scan and diff it against the previous record for
    // the same scan root, so the report can say what improved and what regressed.
    // `--no-history` reads and writes nothing.
    let cur_snapshot = snapshot_of(&summary);
    let cur_files = per_file_history(&report);
    let history_prev = if no_history {
        None
    } else {
        load_history(&scan_root)
    };
    let delta = history_prev
        .as_ref()
        .map(|p| compute_delta(p, &cur_snapshot, &cur_files));

    // Exit code from the FULL offender set (before top truncation).
    let has_warn = report.offenders.iter().any(|o| o.severity == "warn");
    let has_error = report.offenders.iter().any(|o| o.severity == "error");
    let mut exit_code = compute_exit_code(fail_on.as_deref(), has_warn, has_error);

    // `--fail-on-regression`: the ratchet gate. Trips only against a committed
    // baseline; with no baseline there is nothing to regress against, so it warns
    // and passes (the first run, or a repo that has not committed one yet).
    let regression: Option<Vec<String>> = if fail_on_regression {
        match &delta {
            Some(d) => {
                let reasons = regression_reasons(d);
                if !reasons.is_empty() {
                    exit_code = 1;
                }
                Some(reasons)
            }
            None => {
                eprintln!(
                    "  --fail-on-regression: no baseline at {} to compare against - nothing to gate. Commit a baseline first (run `tina4 metrics` and commit the file).",
                    history_path(&scan_root).display()
                );
                Some(Vec::new())
            }
        }
    } else {
        None
    };

    // Regression verdict to stderr, so `--json` keeps a clean machine-readable
    // stdout while a human (and CI) still sees exactly what tripped the gate.
    if let Some(reasons) = &regression {
        if reasons.is_empty() {
            if delta.is_some() {
                eprintln!("  metrics: no regression against the baseline.");
            }
        } else {
            eprintln!("  metrics REGRESSION vs baseline ({} item(s)) - failing:", reasons.len());
            for r in reasons {
                eprintln!("    - {r}");
            }
            eprintln!("  If this is intended, re-baseline: run `tina4 metrics` and commit the updated {}.", HISTORY_FILE);
        }
    }

    let shown: Vec<Offender> = report.offenders.iter().take(top).cloned().collect();

    if json {
        // `--top N` truncates the OFFENDER display only. file_metrics stays
        // complete: the bubble chart must plot every file, and the exit code was
        // already computed from the full offender set above.
        let mut by_cc: Vec<FunctionInfo> = report.functions.clone();
        by_cc.sort_by(|a, b| b.complexity.cmp(&a.complexity));
        by_cc.truncate(15);
        let payload = JsonPayload {
            summary,
            offenders: shown,
            file_metrics: report.files.clone(),
            most_complex_functions: by_cc,
            dependency_graph: report.dependency_graph.clone(),
            duplication: report.clones.clone(),
            unparsed: report.refused.clone(),
            delta,
        };
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()));
        // `--fail-on-regression` is a read-only CHECK against the committed
        // baseline: it never advances the ratchet, so a local check cannot
        // silently move the bar. Re-baselining is a deliberate plain scan.
        if !no_history && !fail_on_regression {
            save_history(&scan_root, history_prev, cur_snapshot, cur_files);
        }
        return exit_code;
    }

    print_human(&summary, &shown, suggest);
    match &delta {
        Some(d) => print_delta_human(d, &scan_root),
        None if !no_history && !fail_on_regression => println!(
            "\n  Baseline saved to {} - rerun to see what changed.",
            history_path(&scan_root).display()
        ),
        None => {}
    }
    if !no_history && !fail_on_regression {
        save_history(&scan_root, history_prev, cur_snapshot, cur_files);
    }
    exit_code
}

fn print_human(summary: &Summary, shown: &[Offender], suggest: bool) {
    use std::io::IsTerminal;
    let use_color = std::io::stdout().is_terminal();
    let paint = |text: &str, code: &str| -> String {
        if use_color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    };

    println!();
    println!("  Tina4 Metrics \u{2014} {} scan ({})", summary.scan_mode, summary.scan_root);
    println!(
        "  files: {}   functions: {}   avg complexity: {}   avg maintainability: {}",
        summary.files_analyzed, summary.total_functions, summary.avg_complexity, summary.avg_maintainability
    );
    if summary.files_refused > 0 {
        println!(
            "  {}",
            paint(
                // Reason-agnostic on purpose: a file is refused for failing to
                // parse OR for nesting deeper than the walks survive, and the
                // per-file reason is on its `unparsed` offender.
                &format!(
                    "! {} file(s) NOT MEASURED - the engine could not read them (see `unparsed` offenders)",
                    summary.files_refused
                ),
                "33"
            )
        );
    }
    if summary.duplicate_blocks > 0 {
        println!(
            "  duplication: {} repeated blocks   {} lines removable by unifying them",
            summary.duplicate_blocks, summary.duplicate_lines
        );
    }
    let showing = if shown.is_empty() { String::new() } else { format!(" (showing top {})", shown.len()) };
    println!("  offenders: {} total{}", summary.total_offenders, showing);
    println!();

    if shown.is_empty() {
        // "clean" and "nothing was looked at" must not print the same thing.
        // Scanning tina4delphi found 39 Pascal files, claimed none of them, and
        // reported a green tick.
        if summary.files_analyzed == 0 {
            println!(
                "  {}",
                paint("no supported source files found - nothing was measured", "33")
            );
        } else {
            println!("  {}", paint("\u{2713} no offenders \u{2014} clean", "32"));
        }
        println!();
        return;
    }

    let loc_cells: Vec<String> = shown.iter().map(|o| format!("{}:{}", o.file, o.line)).collect();
    let loc_w = loc_cells.iter().map(|s| s.len()).chain(std::iter::once("FILE:LINE".len())).max().unwrap_or(9);
    let kind_w = shown.iter().map(|o| o.kind.len()).chain(std::iter::once("KIND".len())).max().unwrap_or(4);

    let header = format!(
        "  {:>3}  {:<8}  {:<kw$}  {:<lw$}  DETAIL",
        "#", "SEVERITY", "KIND", "FILE:LINE", kw = kind_w, lw = loc_w
    );
    println!("{}", paint(&header, "1"));
    println!("  {}", "-".repeat(header.len().saturating_sub(2)));
    for (i, o) in shown.iter().enumerate() {
        let code = match o.severity.as_str() {
            "error" => "31",
            "warn" => "33",
            _ => "2",
        };
        let sev_cell = paint(&format!("{:<8}", o.severity), code);
        println!(
            "  {:>3}  {}  {:<kw$}  {:<lw$}  {}",
            i + 1, sev_cell, o.kind, loc_cells[i], o.detail, kw = kind_w, lw = loc_w
        );
        // --suggest: a dim indented line naming the rule and the move to make.
        // Opt-in so the default table stays byte-identical to before feature 141.
        if suggest && !o.rule_id.is_empty() {
            println!(
                "       {}",
                paint(&format!("\u{2192} {}  {}", o.rule_id, o.remedy), "2")
            );
        }
    }
    println!();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn read_fixture(name: &str) -> String {
        std::fs::read_to_string(manifest().join("tests/fixtures").join(name)).unwrap()
    }

    fn test_index(name: &str, content: &str) -> TestIndex {
        let mut index = TestIndex::default();
        index.add(Path::new(name), content.to_string());
        index
    }

    /// Destructure a MEASURED analysis, or fail loudly.
    ///
    /// A test that quietly accepted a refusal would be asserting nothing at all,
    /// so a refusal panics with the reason attached.
    fn measured(
        lang: Lang,
        src: &str,
        rel: &str,
        has_tests: bool,
    ) -> (FileMetrics, Vec<FunctionInfo>, Vec<String>) {
        match analyze_source(lang, src, rel, has_tests).expect("grammar must load") {
            FileAnalysis::Measured { metrics, functions, imports, .. } => {
                (metrics, functions, imports)
            }
            FileAnalysis::Refused { reason, parse_health } => {
                panic!("{rel}: REFUSED at parse health {parse_health} - {reason}")
            }
        }
    }

    fn analyze_py(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = measured(Lang::Python, src, "t.py", false);
        (fm, fns)
    }

    fn analyze_ts(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = measured(Lang::Ts, src, "t.ts", false);
        (fm, fns)
    }

    fn analyze_php(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = measured(Lang::Php, src, "t.php", false);
        (fm, fns)
    }

    fn analyze_rb(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = measured(Lang::Ruby, src, "t.rb", false);
        (fm, fns)
    }

    fn analyze_rs(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = measured(Lang::Rust, src, "t.rs", false);
        (fm, fns)
    }

    // ---- Rust (ADR-0002 self-measurement) -------------------------------------
    //
    // Until Rust landed, the engine could not measure its OWN implementation
    // language: `tina4 metrics` pointed at this repo found zero files. Every
    // expected number below is hand-derived from the source in the test, so the
    // analyzer is calibrated against McCabe rather than merely self-consistent.

    #[test]
    fn rust_counts_every_decision_kind_once() {
        // Hand count, base 1 plus:
        //   if a > 0                -> if_expression      1
        //   if let Some(v) = b      -> if_expression      1  (NOT a separate kind)
        //   while a > 0             -> while_expression   1
        //   loop { break }          -> loop_expression    1
        //   for i in 0..10          -> for_expression     1
        //   c?                      -> try_expression     1  (the easy one to miss)
        //   a > 0 && a < 10         -> binary &&          1  (`>` and `<` are not)
        // = 8
        let src = "\
fn f(a: i32, b: Option<i32>, c: Result<i32, String>) -> Result<i32, String> {
    if a > 0 { return Ok(1); }
    if let Some(v) = b { let _ = v; }
    while a > 0 { break; }
    loop { break; }
    for i in 0..10 { let _ = i; }
    let _q = c?;
    let _z = a > 0 && a < 10;
    Ok(0)
}
";
        let (fm, fns) = analyze_rs(src);
        assert_eq!(fm.functions, 1);
        assert_eq!(fns[0].complexity, 8, "1 + if + if-let + while + loop + for + ? + &&");
    }

    #[test]
    fn rust_try_operator_is_a_real_branch() {
        // Isolated so a regression in `?` alone cannot hide inside the big count
        // above. Each `?` is an early return, so three of them are three branches.
        let one = "fn f(c: Result<i32, String>) -> Result<i32, String> { Ok(c?) }\n";
        assert_eq!(analyze_rs(one).1[0].complexity, 2, "1 + one ?");
        let three = "fn f(a: Result<i32, String>, b: Result<i32, String>, c: Result<i32, String>) -> Result<i32, String> { Ok(a? + b? + c?) }\n";
        assert_eq!(analyze_rs(three).1[0].complexity, 4, "1 + three ?");
        // The negative half: no `?` means no extra branch.
        let none = "fn f(c: i32) -> i32 { c }\n";
        assert_eq!(analyze_rs(none).1[0].complexity, 1);
    }

    #[test]
    fn rust_else_is_not_a_decision_but_else_if_is() {
        // `if/else` is ONE decision - `else` is the fall-through edge. `else if`
        // nests a second if_expression and so is a second decision. Counting
        // `else_clause` would score these 3 and 4 instead of 2 and 3.
        let if_else = "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n";
        assert_eq!(analyze_rs(if_else).1[0].complexity, 2, "if/else = 1 decision");
        let chain = "fn f(a: i32) -> i32 { if a > 0 { 1 } else if a < 0 { 2 } else { 3 } }\n";
        assert_eq!(analyze_rs(chain).1[0].complexity, 3, "if / else-if / else = 2 decisions");
    }

    #[test]
    fn rust_wildcard_match_arm_is_not_a_decision() {
        // Mirrors the TypeScript precedent: tree-sitter names `default:`
        // `switch_default`, so the engine has never counted it. `_` is the same
        // fall-through, so three arms with a wildcard are two decisions.
        let src = "fn f(a: i32) -> i32 { match a { 1 => 1, 2 => 2, _ => 0 } }\n";
        assert_eq!(analyze_rs(src).1[0].complexity, 3, "1 + two real arms, wildcard excluded");
        // Without a wildcard every arm counts.
        let exhaustive = "fn f(a: bool) -> i32 { match a { true => 1, false => 0 } }\n";
        assert_eq!(analyze_rs(exhaustive).1[0].complexity, 3, "1 + two arms");
    }

    #[test]
    fn rust_closure_is_its_own_callable_scope() {
        let src = "fn f(xs: Vec<i32>) -> Vec<i32> { xs.into_iter().map(|x| if x > 0 { 1 } else { 0 }).collect() }\n";
        let (fm, fns) = analyze_rs(src);
        assert_eq!(fm.functions, 2, "the closure is a callable of its own");
        assert_eq!(fns[0].complexity, 1, "the outer function owns no decision");
        assert_eq!(fns[1].complexity, 2, "the closure owns its if");
    }

    #[test]
    fn rust_impl_method_is_named_for_its_type() {
        // impl_item stores the type under a field called `type`, NOT `name`.
        // Without the node_name fallback every method reported bare.
        let inherent = "struct Point;\nimpl Point { fn new() -> Self { Point } }\n";
        let (_fm, fns) = analyze_rs(inherent);
        assert_eq!(fns[0].name, "Point.new");

        let trait_impl = "struct Point;\ntrait Draw { fn draw(&self); }\nimpl Draw for Point { fn draw(&self) {} }\n";
        let (_fm, fns) = analyze_rs(trait_impl);
        assert!(
            fns.iter().any(|f| f.name == "Point.draw"),
            "a trait impl is named for the implementing TYPE, not the trait: {:?}",
            fns.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rust_loc_excludes_line_doc_and_block_comments() {
        let src = "\
// a line comment
/// a doc comment
//! an inner doc comment
/* a block comment
 * continued
 */
fn f() -> i32 {
    1
}
";
        let (fm, _fns) = analyze_rs(src);
        // Only `fn f() -> i32 {`, `1` and `}` are code.
        assert_eq!(fm.loc, 3, "six comment lines must not count as code");
    }

    /// NEGATIVE CONTROL. A short, clean, low-complexity Rust file must score
    /// HIGH and raise no offender. Without this, a change that simply broke the
    /// Rust analyzer - making everything look complex and unmaintainable - would
    /// pass every assertion above, since those only check that numbers are large.
    #[test]
    fn rust_negative_control_a_clean_file_scores_clean() {
        let src = "\
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn sub(a: i32, b: i32) -> i32 {
    a - b
}
";
        // has_tests: true, because `untested` is a fact about the fixture, not
        // about the analyzer, and this control is asserting that the ANALYSIS is
        // clean.
        let (fm, fns, _s) = measured(Lang::Rust, src, "clean.rs", true);
        assert_eq!(fm.functions, 2);
        assert!(fns.iter().all(|f| f.complexity == 1), "straight-line code is CC 1");
        assert!(
            fm.maintainability > 60.0,
            "a trivial module must score HIGH (well clear of the 40 offender line), got {}",
            fm.maintainability
        );
        assert!(
            build_offenders(&[fm], &fns).is_empty(),
            "a clean file must raise no offender at all"
        );
    }

    /// Calibration, not self-consistency: the SAME two trivial functions written
    /// in all five languages must land in the same maintainability band.
    ///
    /// This is what catches a Rust analyzer that is internally coherent but
    /// systematically wrong - if the decision set or the operand-leaf list were
    /// badly off, Rust's MI would drift away from the other four and every
    /// number in the audit would be quietly mis-scaled. Measured at the time of
    /// writing: rust 70.4, ts 72.4, ruby 72.4, php 71.0.
    ///
    /// Python is excluded from the band on purpose: it alone uses the ast-exact
    /// `py_halstead` (volume 12 here against the generic walk's 31) because it
    /// alone has a cross-language parity target, so it legitimately scores
    /// higher and is not evidence of anything about Rust.
    #[test]
    fn rust_maintainability_is_in_band_with_the_other_languages() {
        let cases = [
            (
                "rust",
                Lang::Rust,
                "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n",
            ),
            (
                "ts",
                Lang::Ts,
                "export function add(a: number, b: number): number {\n    return a + b;\n}\n\nexport function sub(a: number, b: number): number {\n    return a - b;\n}\n",
            ),
            (
                "php",
                Lang::Php,
                "<?php\nfunction add($a, $b) {\n    return $a + $b;\n}\n\nfunction sub($a, $b) {\n    return $a - $b;\n}\n",
            ),
            (
                "ruby",
                Lang::Ruby,
                "def add(a, b)\n  a + b\nend\n\ndef sub(a, b)\n  a - b\nend\n",
            ),
        ];
        let mut scores = Vec::new();
        for (name, lang, src) in cases {
            let (fm, _f, _s) = measured(lang, src, "t", false);
            assert_eq!(fm.functions, 2, "{name}: both named functions must be found");
            scores.push((name, fm.maintainability));
        }
        let rust = scores.iter().find(|(n, _)| *n == "rust").unwrap().1;
        for (name, mi) in &scores {
            assert!(
                (rust - mi).abs() < 8.0,
                "rust MI {rust} is out of band with {name} MI {mi} for identical logic \
                 - the Rust decision set or operand list has drifted"
            );
        }
    }

    #[test]
    fn rust_mod_and_crate_paths_resolve_to_real_files() {
        let src = "mod agent;\npub mod console;\nuse crate::console::icon_ok;\nuse std::fs;\nuse serde::Serialize;\n";
        let (_fm, _fns, specs) = measured(Lang::Rust, src, "main.rs", false);
        assert!(specs.contains(&"mod:agent".to_string()), "got {specs:?}");
        assert!(specs.contains(&"mod:console".to_string()), "got {specs:?}");
        assert!(specs.contains(&"crate::console::icon_ok".to_string()), "got {specs:?}");

        let paths = pathset(&["main.rs", "agent.rs", "console.rs"]);
        assert_eq!(
            resolve_import("mod:agent", "main.rs", Lang::Rust, &paths, None),
            Some("agent.rs".to_string())
        );
        assert_eq!(
            resolve_import("crate::console::icon_ok", "main.rs", Lang::Rust, &paths, None),
            Some("console.rs".to_string()),
            "the item tail is dropped once the module prefix matches a file"
        );
        // The negative half, and the one that matters most: an EXTERNAL crate
        // must not resolve, or every third-party import becomes fake internal
        // coupling and instability collapses to a constant.
        //
        // The names are chosen so the assertion can actually FAIL. `console` and
        // `session` are both real crates on crates.io AND real files in this
        // repo, so a resolver that ignores the leading segment would happily
        // bind `use console::Term` to the local console.rs and invent an edge
        // that does not exist. Asserting against `std::fs` alone proves nothing
        // here - there is no std.rs to collide with, so it returns None whether
        // the rule is right or wrong.
        assert_eq!(
            resolve_import("console::Term", "main.rs", Lang::Rust, &paths, None),
            None,
            "a BARE first segment is an external crate even when a local file shares its name"
        );
        assert_eq!(
            resolve_import("crate::console::Term", "main.rs", Lang::Rust, &paths, None),
            Some("console.rs".to_string()),
            "the same path via `crate::` IS the local file - this is the contrast that makes the rule meaningful"
        );
        assert_eq!(resolve_import("std::fs", "main.rs", Lang::Rust, &paths, None), None);
        assert_eq!(resolve_import("serde::Serialize", "main.rs", Lang::Rust, &paths, None), None);
    }

    /// Adding a language must not move the FOUR that already shipped.
    ///
    /// The live hazard was concrete: Rust needs `primitive_type` treated as a
    /// Halstead operand, and tree-sitter-php emits `primitive_type` too (for
    /// `int` / `string` type hints). Folding the Rust leaf kinds into the shared
    /// list would have shifted every PHP file's volume, and therefore its MI,
    /// with nothing in the suite to notice - the parity lock covers Python only.
    ///
    /// So the Rust kinds are gated on the language, and these are the exact
    /// volumes the generic walk produced BEFORE Rust existed. If a future change
    /// widens the shared list, this test goes red instead of the audit going
    /// quietly wrong.
    #[test]
    fn adding_rust_does_not_perturb_the_other_languages() {
        for (name, lang, src) in [
            (
                "ts",
                Lang::Ts,
                "export function add(a: number, b: number): number {\n    return a + b;\n}\n\nexport function sub(a: number, b: number): number {\n    return a - b;\n}\n",
            ),
            (
                "php",
                Lang::Php,
                "<?php\nfunction add($a, $b) {\n    return $a + $b;\n}\n\nfunction sub($a, $b) {\n    return $a - $b;\n}\n",
            ),
            (
                "ruby",
                Lang::Ruby,
                "def add(a, b)\n  a + b\nend\n\ndef sub(a, b)\n  a - b\nend\n",
            ),
        ] {
            let (fm, _f, _s) = measured(lang, src, "t", false);
            assert_eq!(
                fm.halstead_volume, 31.02,
                "{name}: Halstead volume moved - the Rust operand kinds have leaked \
                 into the shared list and every {name} MI in the audit is now wrong"
            );
        }
        // A PHP typed signature is the specific collision: `int` parses as
        // `primitive_type`, which Rust counts and PHP must NOT.
        let typed = "<?php\nfunction add(int $a, int $b): int {\n    return $a + $b;\n}\n";
        let (fm, _f, _s) = measured(Lang::Php, typed, "t.php", false);
        assert_eq!(
            fm.halstead_volume, 12.0,
            "PHP `primitive_type` must stay uncounted"
        );
    }

    /// Pascal / Delphi is deliberately NOT wired up, and that is a measured
    /// decision rather than an omission.
    ///
    /// The only published crate, `tree-sitter-pascal` 0.10.2 (Isopod), compiles
    /// against tree-sitter 0.26 and handles classic Object Pascal well, but it
    /// cannot parse Delphi 10.3+ inline loop variables (`for var X in Y do`,
    /// upstream issue #15, still open). Measured against the real tina4delphi
    /// corpus: 20,977 of 40,719 lines - 51.5% - fall inside an ERROR region,
    /// including 100% of Tina4Frond.pas and 92.8% of Tina4HTMLRender.pas.
    ///
    /// Claiming `.pas` would mean emitting an MI derived from a tree where half
    /// the decision points are invisible. A missing number is recoverable; a
    /// confident wrong one is not. So `.pas` stays unrecognised until a grammar
    /// that parses the corpus is available.
    #[test]
    fn pascal_is_not_claimed() {
        for ext in ["pas", "dpr", "dpk", "inc", "PAS"] {
            assert_eq!(
                Lang::from_path(Path::new(&format!("a.{ext}"))),
                None,
                "{ext} must not be claimed while no grammar can parse the corpus"
            );
        }
    }

    #[test]
    fn rust_inline_cfg_test_module_counts_as_tested() {
        // Rust puts unit tests inside the file. Every stage of module_has_tests
        // looks for an external test FILE, so without this every .rs file in the
        // repo raised a false `untested` offender.
        assert!(rust_has_inline_tests("fn f() {}\n#[cfg(test)]\nmod tests { #[test] fn t() {} }\n"));
        // The negative half: a file with no test module is genuinely untested.
        assert!(!rust_has_inline_tests("fn f() -> i32 { 1 }\n"));
        assert!(
            !rust_has_inline_tests("// mentions cfg and test but declares neither\n"),
            "the marker is the attribute, not the words"
        );
    }

    // ---- Language-agnostic building blocks -----------------------------------

    #[test]
    fn detects_language_from_extension() {
        for ext in ["py", "pyw"] {
            assert_eq!(Lang::from_path(Path::new(&format!("a.{ext}"))), Some(Lang::Python));
        }
        assert_eq!(Lang::from_path(Path::new("a.php")), Some(Lang::Php));
        assert_eq!(Lang::from_path(Path::new("a.rb")), Some(Lang::Ruby));
        for ext in ["ts", "tsx", "js", "jsx", "mjs"] {
            assert_eq!(Lang::from_path(Path::new(&format!("a.{ext}"))), Some(Lang::Ts));
        }
        assert_eq!(Lang::from_path(Path::new("a.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(Path::new("A.RS")), Some(Lang::Rust), "extension match is case-insensitive");
        assert_eq!(Lang::from_path(Path::new("a.md")), None);
        assert_eq!(Lang::from_path(Path::new("noext")), None);
        // Pascal is deliberately NOT wired up - see the `pascal_is_not_claimed`
        // test below for why claiming it would be worse than omitting it.
        assert_eq!(Lang::from_path(Path::new("a.pas")), None);
    }

    #[test]
    fn cyclomatic_complexity_counts_decision_points_python() {
        // 1 (base) + if + boolean_operator(and) + for = 4. A comparison is NOT a
        // decision point (matches metrics.py), so `x > 0` adds nothing.
        let src = "def f(x):\n    if x and x > 0:\n        for i in x:\n            pass\n    return x\n";
        let (fm, fns) = analyze_py(src);
        assert_eq!(fm.functions, 1);
        assert_eq!(fns[0].complexity, 4, "CC should be 1+if+and+for");
        assert_eq!(fm.complexity, 4);
    }

    #[test]
    fn comprehension_and_ternary_add_complexity_python() {
        // 1 + for_in_clause + if_clause + conditional_expression(ternary) = 4.
        let src = "def g(items):\n    xs = [n for n in items if n]\n    return 1 if items else 0\n";
        let (_fm, fns) = analyze_py(src);
        assert_eq!(fns[0].complexity, 4);
    }

    #[test]
    fn maintainability_index_is_bounded_and_named_with_class() {
        let src = "class Foo:\n    def bar(self):\n        return 1\n";
        let (fm, fns) = analyze_py(src);
        assert!(fm.maintainability >= 0.0 && fm.maintainability <= 100.0);
        assert_eq!(fns[0].name, "Foo.bar", "method name carries its class prefix");
    }

    #[test]
    fn empty_file_scores_full_maintainability() {
        let (fm, _f) = analyze_py("\n\n# just a comment\n");
        assert_eq!(fm.loc, 0);
        assert_eq!(fm.maintainability, 100.0);
        assert_eq!(fm.functions, 0);
    }

    // ---- Nested scopes are measured once, not twice --------------------------

    #[test]
    fn a_parent_is_not_charged_for_its_nested_functions_python() {
        // `outer` has NO branch of its own. Each inner has two. Before the fix
        // outer reported 5 - its own base plus all four inner branches.
        let src = "def outer(a):\n    def inner1(x):\n        if x: return 1\n        if x > 2: return 2\n        return 3\n    def inner2(y):\n        if y: return 1\n        if y > 2: return 2\n        return 3\n    return inner1(a) + inner2(a)\n";
        let (fm, fns) = analyze_py(src);
        let outer = fns.iter().find(|f| f.name == "outer").unwrap();
        assert_eq!(outer.complexity, 1, "outer branches on nothing itself");
        for name in ["inner1", "inner2"] {
            let f = fns.iter().find(|f| f.name == name).unwrap();
            assert_eq!(f.complexity, 3, "{name} keeps its own two branches");
        }
        // The file total is the sum of the per-function values, so it drops too.
        assert_eq!(fm.complexity, 7, "1 + 3 + 3, with nothing counted twice");
    }

    #[test]
    fn a_python_lambda_is_its_own_callable_scope() {
        let src = "def f(xs):\n    return sorted(xs, key=lambda x: 1 if x else 0)\n";
        let (_fm, fns) = analyze_py(src);
        assert_eq!(fns.len(), 2, "the lambda is reported separately");
        assert_eq!(fns[0].complexity, 1, "the outer function owns no decision");
        assert_eq!(fns[1].complexity, 2, "the lambda owns its ternary");
    }

    #[test]
    fn a_method_in_a_nested_class_is_not_charged_to_the_outer_function() {
        let src = "def make():\n    class Inner:\n        def go(self, x):\n            if x: return 1\n            return 2\n    return Inner\n";
        let (_fm, fns) = analyze_py(src);
        let outer = fns.iter().find(|f| f.name == "make").unwrap();
        assert_eq!(outer.complexity, 1, "the nested class body is a separate scope");
        let go = fns.iter().find(|f| f.name.ends_with("go")).unwrap();
        assert_eq!(go.complexity, 2);
    }

    #[test]
    fn an_iife_wrapper_does_not_absorb_the_whole_module_typescript() {
        // The real symptom: public/js/frond.js reported cc=191 because the whole
        // file sat inside one anonymous wrapper.
        let src = "(function () {\n  function a(x) { if (x) { return 1; } return 2; }\n  function b(y) { if (y) { return 1; } return 2; }\n  return { a: a, b: b };\n})();\n";
        let (_fm, fns) = analyze_ts(src);
        let wrapper = fns.iter().min_by_key(|f| f.line).unwrap();
        assert_eq!(wrapper.complexity, 1, "the wrapper itself branches on nothing");
        assert!(fns.iter().any(|f| f.complexity == 2), "inner functions keep theirs");
    }

    #[test]
    fn ruby_keyword_tokens_are_not_counted_as_decisions() {
        // Regression: tree-sitter's Ruby grammar uses "if"/"unless"/"while"/"when"
        // for both the construct and its keyword token, so every decision was
        // counted twice and Ruby complexity came out roughly double. Verified
        // against metrics.rb, which reports 2 for each of these.
        for (src, expected, what) in [
            ("def m(y)\n  return 1 if y\n  2\nend\n", 2, "modifier if"),
            ("def m(z)\n  if z\n    1\n  else\n    2\n  end\nend\n", 2, "if/else"),
            ("def m(z)\n  while z\n    z -= 1\n  end\nend\n", 2, "while"),
            ("def m(z)\n  z.each { |i| puts i }\n  1\nend\n", 1, "a block is not a decision"),
        ] {
            let (_fm, fns) = analyze_rb(src);
            assert_eq!(fns[0].complexity, expected, "{what}");
        }
    }

    #[test]
    fn a_php_closure_is_its_own_callable_scope() {
        let php = "<?php\nclass A {\n  function outer($x) {\n    return array_map(function ($y) { if ($y) { return 1; } return 2; }, $x);\n  }\n}\n";
        let (_fm, fns) = analyze_php(php);
        let outer = fns.iter().find(|f| f.name.ends_with("outer")).unwrap();
        assert_eq!(outer.complexity, 1, "the outer method owns no decision");
        let closure = fns.iter().find(|f| f.name.contains("anonymous")).unwrap();
        assert_eq!(closure.complexity, 2, "the closure owns its if");
    }

    #[test]
    fn nested_callable_boundaries_match_in_all_languages() {
        let cases = [
            (Lang::Python, "def register(items):\n    first = lambda x: 1 if x else 0\n    second = lambda x: 2 if x else 0\n    return first(items[0]) + second(items[0])\n"),
            (Lang::Php, "<?php\nfunction register($items) {\n  $first = fn($x) => $x ? 1 : 0;\n  $second = fn($x) => $x ? 2 : 0;\n  return $first($items[0]) + $second($items[0]);\n}\n"),
            (Lang::Ruby, "def register(items)\n  first = ->(x) { x ? 1 : 0 }\n  second = ->(x) { x ? 2 : 0 }\n  first.call(items[0]) + second.call(items[0])\nend\n"),
            (Lang::Ts, "function register(items: number[]) {\n  const first = (x: number) => x ? 1 : 0;\n  const second = (x: number) => x ? 2 : 0;\n  return first(items[0]) + second(items[0]);\n}\n"),
            (Lang::Rust, "fn register(items: Vec<i32>) -> i32 {\n  let first = |x: i32| if x > 0 { 1 } else { 0 };\n  let second = |x: i32| if x > 0 { 2 } else { 0 };\n  first(items[0]) + second(items[0])\n}\n"),
        ];
        for (lang, source) in cases {
            let (_metrics, functions, _imports) = measured(lang, source, "scope", true);
            assert_eq!(functions.len(), 3, "{lang:?}: outer and nested callables must be separate: {functions:?}");
            assert_eq!(functions[0].complexity, 1, "{lang:?}: outer callable must not absorb nested decisions");
            assert_eq!(functions[1].complexity, 2, "{lang:?}: first nested callable owns its decision");
            assert_eq!(functions[2].complexity, 2, "{lang:?}: second nested callable owns its decision");
        }
    }

    #[test]
    fn nested_methods_do_not_double_count_php_and_ruby() {
        // A named function declared inside a method IS listed separately, so its
        // branch must not also land on the method.
        let php = "<?php\nclass A {\n  function outer($x) {\n    function helper($y) { if ($y) { return 1; } return 2; }\n    return helper($x);\n  }\n}\n";
        let (_fm, fns) = analyze_php(php);
        let outer = fns.iter().find(|f| f.name.ends_with("outer")).unwrap();
        assert_eq!(outer.complexity, 1, "helper's branch belongs to helper");
        let helper = fns.iter().find(|f| f.name.ends_with("helper")).unwrap();
        assert_eq!(helper.complexity, 2);

        let rb = "class A\n  def outer(x)\n    inner(x)\n  end\n  def inner(y)\n    return 1 if y\n    2\n  end\nend\n";
        let (_fm, fns) = analyze_rb(rb);
        let outer = fns.iter().find(|f| f.name.ends_with("outer")).unwrap();
        assert_eq!(outer.complexity, 1);
        let inner = fns.iter().find(|f| f.name.ends_with("inner")).unwrap();
        assert_eq!(inner.complexity, 2);
    }

    // ---- Offender rules (mirror metrics.py thresholds) -----------------------

    fn file_with(mi: f64, loc: usize, funcs: usize, has_tests: bool) -> FileMetrics {
        // avg_complexity 8.0: a genuinely complex file, so the low-MI rule (now
        // complexity-gated) still fires for the tests that assert it does.
        file_with_cc(mi, loc, funcs, has_tests, 8.0)
    }

    fn file_with_cc(mi: f64, loc: usize, funcs: usize, has_tests: bool, avg_cc: f64) -> FileMetrics {
        FileMetrics {
            path: "x.py".into(), loc, complexity: (avg_cc * funcs as f64) as u32,
            avg_complexity: avg_cc,
            functions: funcs, maintainability: mi, halstead_volume: 0.0,
            has_referencing_test: has_tests,
            dep_count: 0, coupling_efferent: 0, coupling_afferent: 0, instability: 0.0,
            parse_health: 1.0,
        }
    }
    fn func_with(cc: u32) -> FunctionInfo {
        FunctionInfo { name: "f".into(), file: "x.py".into(), line: 1, complexity: cc, loc: 1 }
    }
    fn func_with_loc(cc: u32, loc: usize) -> FunctionInfo {
        FunctionInfo { name: "f".into(), file: "x.py".into(), line: 1, complexity: cc, loc }
    }

    #[test]
    fn offender_low_maintainability_severity_split_at_20() {
        let warn = build_offenders(&[file_with(35.0, 10, 1, true)], &[]);
        assert_eq!(warn[0].kind, "low_maintainability");
        assert_eq!(warn[0].severity, "warn");
        let err = build_offenders(&[file_with(15.0, 10, 1, true)], &[]);
        assert_eq!(err[0].severity, "error");
        // MI >= 40 produces no low_maintainability offender.
        let clean = build_offenders(&[file_with(55.0, 10, 1, true)], &[]);
        assert!(clean.iter().all(|o| o.kind != "low_maintainability"));
    }

    #[test]
    fn low_maintainability_does_not_fire_on_a_big_but_simple_file() {
        // The false positive this rule shipped with. A large file of trivial,
        // branchless functions (avg CC 1.0) scores a low MI purely because the
        // formula is size-dominated - MEASURED at MI 8.4 for 400 branchless
        // functions over 1200 lines. That is size, not unmaintainability, and
        // `large_file` already reports it. The low-MI ERROR was noise on top.
        let big_simple = file_with_cc(8.0, 1200, 400, true, 1.0);
        let offs = build_offenders(&[big_simple], &[]);
        assert!(
            offs.iter().all(|o| o.kind != "low_maintainability"),
            "low_maintainability fired on a big-but-simple file (avg CC 1.0) - it is re-flagging size"
        );
        // ...but large_file still catches the real property (it IS large).
        assert!(offs.iter().any(|o| o.kind == "large_file"));
    }

    #[test]
    fn low_maintainability_still_fires_when_functions_are_complex() {
        // The signal that must survive the gate: low MI AND genuinely complex
        // functions (avg CC 8) is a real maintainability problem.
        let complex = file_with_cc(15.0, 1200, 400, true, 8.0);
        let offs = build_offenders(&[complex], &[]);
        let mi = offs.iter().find(|o| o.kind == "low_maintainability");
        assert!(mi.is_some(), "a low-MI file with complex functions must still flag");
        assert_eq!(mi.unwrap().severity, "error");
    }

    #[test]
    fn offender_complexity_severity_split_at_20() {
        let warn = build_offenders(&[], &[func_with(15)]);
        assert_eq!(warn[0].kind, "complexity");
        assert_eq!(warn[0].severity, "warn");
        let err = build_offenders(&[], &[func_with(25)]);
        assert_eq!(err[0].severity, "error");
        // CC <= 10 is not an offender.
        assert!(build_offenders(&[], &[func_with(10)]).is_empty());
    }

    // ── Feature 141: rule ids + remedies ────────────────────────────────────

    #[test]
    fn complexity_splits_into_long_method_vs_dense_logic_by_loc() {
        // A long function (loc >= 50) with high cc is a LONG METHOD -> M001.
        let long = build_offenders(&[], &[func_with_loc(25, 80)]);
        assert_eq!(long[0].kind, "complexity");
        assert_eq!(long[0].rule_id, "TINA4-M001");
        assert!(long[0].remedy.contains("extract"));
        // A short-but-branchy function is DENSE LOGIC -> M002, never M001.
        let dense = build_offenders(&[], &[func_with_loc(25, 12)]);
        assert_eq!(dense[0].rule_id, "TINA4-M002");
        assert_ne!(dense[0].rule_id, "TINA4-M001", "a short dense function must not read as a long method");
        assert!(dense[0].remedy.contains("guard clauses") || dense[0].remedy.contains("dispatch"));
        // The boundary is LONG_METHOD_LOC: exactly 50 is long, 49 is dense.
        assert_eq!(build_offenders(&[], &[func_with_loc(15, 50)])[0].rule_id, "TINA4-M001");
        assert_eq!(build_offenders(&[], &[func_with_loc(15, 49)])[0].rule_id, "TINA4-M002");
    }

    #[test]
    fn each_file_level_kind_maps_to_its_stable_rule_id() {
        // large_file -> S020
        let big = build_offenders(&[file_with(90.0, 900, 1, true)], &[]);
        let lf = big.iter().find(|o| o.kind == "large_file").unwrap();
        assert_eq!(lf.rule_id, "TINA4-S020");
        assert!(lf.remedy.contains("split"));
        // too_many_functions -> S021
        let many = build_offenders(&[file_with(90.0, 10, 21, true)], &[]);
        assert_eq!(many.iter().find(|o| o.kind == "too_many_functions").unwrap().rule_id, "TINA4-S021");
        // low_maintainability -> S022
        let lowmi = build_offenders(&[file_with(15.0, 10, 1, true)], &[]);
        assert_eq!(lowmi.iter().find(|o| o.kind == "low_maintainability").unwrap().rule_id, "TINA4-S022");
        // no_test_reference -> C101 (info)
        let untested = build_offenders(&[file_with(90.0, 10, 1, false)], &[]);
        let nt = untested.iter().find(|o| o.kind == "no_test_reference").unwrap();
        assert_eq!(nt.rule_id, "TINA4-C101");
        assert_eq!(nt.severity, "info");
    }

    #[test]
    fn rule_for_is_the_frozen_catalogue() {
        // Pin the published ids so a recycle/repoint is a test failure (the
        // never-recycle contract). loc only matters for complexity.
        assert_eq!(rule_for("complexity", 80).0, "TINA4-M001");
        assert_eq!(rule_for("complexity", 10).0, "TINA4-M002");
        assert_eq!(rule_for("duplication", 0).0, "TINA4-D010");
        assert_eq!(rule_for("large_file", 0).0, "TINA4-S020");
        assert_eq!(rule_for("too_many_functions", 0).0, "TINA4-S021");
        assert_eq!(rule_for("low_maintainability", 0).0, "TINA4-S022");
        assert_eq!(rule_for("no_test_reference", 0).0, "TINA4-C101");
        assert_eq!(rule_for("unparsed", 0).0, "TINA4-U901");
        // Every assigned rule carries a non-empty remedy; an unknown kind gets
        // NO id rather than a wrong one.
        for kind in ["complexity","duplication","large_file","too_many_functions","low_maintainability","no_test_reference","unparsed"] {
            let (id, remedy) = rule_for(kind, 0);
            assert!(!id.is_empty() && !remedy.is_empty(), "{kind} must have an id and remedy");
        }
        assert_eq!(rule_for("something_new", 0), ("", ""));
    }

    #[test]
    fn rule_id_and_remedy_serialise_into_json() {
        let offs = build_offenders(&[], &[func_with_loc(25, 80)]);
        let value = serde_json::to_value(&offs[0]).unwrap();
        assert_eq!(value["rule_id"], "TINA4-M001");
        assert!(value["remedy"].as_str().unwrap().contains("extract"));
    }

    #[test]
    fn duplication_offender_carries_its_rule_id() {
        let group = CloneGroup {
            first_file: "a.py".into(), first_line: 1, lines: 40, copies: 2,
            cross_file: true, files: vec!["a.py".into(), "b.py".into()],
            occurrences: vec![],
        };
        let offs = clone_offenders(&[group]);
        assert_eq!(offs[0].rule_id, "TINA4-D010");
        assert!(offs[0].remedy.contains("unify"));
    }

    #[test]
    fn offender_too_many_functions_and_no_test_reference() {
        let offs = build_offenders(&[file_with(90.0, 10, 21, false)], &[]);
        assert!(offs.iter().any(|o| o.kind == "too_many_functions" && o.severity == "warn"));
        assert!(offs.iter().any(|o| o.kind == "no_test_reference" && o.severity == "info"));
        // Exactly 20 functions and a matched test => neither offender.
        let clean = build_offenders(&[file_with(90.0, 10, 20, true)], &[]);
        assert!(clean.is_empty());
    }

    /// Named regression (parity with Python master fee4385). The old code capped
    /// the complexity offenders to the 15 most-complex functions (`by_cc.take(15)`),
    /// so a file with >15 over-threshold functions silently dropped the 16th+ from
    /// the offenders list AND from `--fail-on` — a too-complex function passed the
    /// gate. No mocks: builds REAL Python source with 18 functions each at
    /// cyclomatic complexity > 20 and drives it through the real tree-sitter
    /// analyzer, then asserts every one surfaces as an "error" complexity offender.
    #[test]
    fn offender_complexity_not_capped_all_over_threshold_surface() {
        let n: usize = 18; // > 15, so the old [:15] cap would have dropped fn15..fn17
        // 24 independent `if` statements => cyclomatic complexity = 1 + 24 = 25 (> 20).
        let body: String = (0..24).map(|j| format!("    if x == {j}:\n        x += 1\n")).collect();
        let src: String = (0..n)
            .map(|i| format!("def fn{i}(x):\n{body}    return x\n"))
            .collect::<Vec<_>>()
            .join("\n");

        let (_fm, fns) = analyze_py(&src);
        assert_eq!(fns.len(), n, "all {n} functions must parse");
        assert!(fns.iter().all(|f| f.complexity == 25), "each function is CC 25 (1 + 24 ifs)");

        let offs = build_offenders(&[], &fns);
        let complexity: Vec<&Offender> = offs.iter().filter(|o| o.kind == "complexity").collect();
        // Old capped behaviour surfaced exactly 15; the fix surfaces all 18.
        assert_eq!(
            complexity.len(),
            n,
            "expected {n} complexity offenders (was capped at 15), got {}",
            complexity.len()
        );
        assert!(
            complexity.iter().all(|o| o.severity == "error"),
            "CC 25 > 20 => every complexity offender is severity error"
        );
    }

    #[test]
    fn fail_on_gate_exit_codes() {
        assert_eq!(compute_exit_code(None, true, true), 0);
        assert_eq!(compute_exit_code(Some("warn"), false, false), 0);
        assert_eq!(compute_exit_code(Some("warn"), true, false), 1);
        assert_eq!(compute_exit_code(Some("warn"), false, true), 1);
        assert_eq!(compute_exit_code(Some("error"), true, false), 0);
        assert_eq!(compute_exit_code(Some("error"), false, true), 1);
    }

    #[test]
    fn regression_gate_reasons() {
        let snap = |offenders: usize, dup_lines: usize| MetricsSnapshot {
            at: 0,
            tool_version: "test".into(),
            files_analyzed: 1,
            total_functions: 1,
            avg_complexity: 1.0,
            avg_maintainability: 100.0,
            total_offenders: offenders,
            duplicate_blocks: 0,
            duplicate_lines: dup_lines,
        };
        let fd = |path: &str, ob: i64, oa: i64, wb: i64, wa: i64, status: &str| FileDelta {
            path: path.into(),
            offenders_before: ob,
            offenders_after: oa,
            worst_cc_before: wb,
            worst_cc_after: wa,
            metrics_before: None,
            metrics_after: None,
            status: status.into(),
        };
        let base = |regressed: Vec<FileDelta>, before_dup: usize, after_dup: usize| RunDelta {
            since_secs: 1,
            before: snap(0, before_dup),
            after: snap(0, after_dup),
            improved_files: vec![],
            regressed_files: regressed,
            changed_files: vec![],
        };

        // Clean: no regressed files, duplication unchanged -> no reasons -> passes.
        assert!(regression_reasons(&base(vec![], 100, 100)).is_empty());

        // A file that gained an offender -> one reason naming the offender move.
        let r = regression_reasons(&base(vec![fd("orm.rb", 0, 1, 5, 41, "regressed")], 0, 0));
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("orm.rb") && r[0].contains("offenders 0->1") && r[0].contains("41"));

        // A brand-new file arriving WITH an offender -> flagged as NEW.
        let r = regression_reasons(&base(vec![fd("new.py", 0, 2, 0, 30, "new")], 0, 0));
        assert!(r[0].contains("new.py") && r[0].contains("NEW file"));

        // Duplication is the summary axis the per-file pass never gates: a rise
        // in duplicated lines alone must still trip the gate.
        let r = regression_reasons(&base(vec![], 100, 143));
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("duplicated lines 100->143"));

        // A DROP in duplication is not a regression.
        assert!(regression_reasons(&base(vec![], 143, 100)).is_empty());
    }

    // ---- Non-Python: proves no framework / no project needed -----------------

    #[test]
    fn analyzes_php_ruby_typescript_without_a_project() {
        let php = "<?php\nfunction f($a){ if ($a && $a > 0) { return 1; } return 0; }\n";
        let (fm, fns, _s) = measured(Lang::Php, php, "t.php", false);
        assert_eq!(fm.functions, 1);
        assert!(fns[0].complexity >= 3); // 1 + if + &&
        assert!(fm.maintainability > 0.0 && fm.maintainability <= 100.0);

        let rb = "def f(a)\n  return 1 if a && a > 0\n  0\nend\n";
        let (fm, _f, _s) = measured(Lang::Ruby, rb, "t.rb", false);
        assert_eq!(fm.functions, 1);

        let ts = "export const f = (a: number) => { if (a && a > 0) { return 1; } return 0; };\n";
        let (fm, fns, _s) = measured(Lang::Ts, ts, "t.ts", false);
        assert_eq!(fm.functions, 1, "top-level arrow function is counted");
        assert!(fns[0].complexity >= 3);
    }

    #[test]
    fn typescript_imports_are_counted_as_dep_count() {
        let ts = "import { a } from './a';\nimport b from './b';\nconst x = () => a + b;\n";
        let (fm, _f, specs) = measured(Lang::Ts, ts, "t.ts", false);
        // dep_count is every import as written - the number the dashboard badges.
        assert_eq!(fm.dep_count, 2);
        // Order is not part of the contract: the walk is a stack-based DFS and the
        // graph's targets are sorted downstream, so compare as a set.
        let mut got = specs.clone();
        got.sort();
        assert_eq!(got, vec!["./a".to_string(), "./b".to_string()]);
        // The coupling TRIPLE is internal-only and needs the whole file set, so
        // analyze_source leaves it at zero; analyze_targets fills it in.
        assert_eq!(fm.coupling_efferent, 0, "resolved in pass 2, not here");
    }

    #[test]
    fn php_inline_fully_qualified_references_are_dependencies() {
        // PHP resolves most dependencies through the autoloader from an INLINE
        // fully-qualified name, not a `use` statement. In the real framework that
        // is 243 inline references against 72 `use` lines, so counting only `use`
        // understated PHP coupling by 3-4x (40 edges instead of 138).
        let php = "<?php\nnamespace Tina4;\nfunction boot() {\n    \\Tina4\\DotEnv::load();\n    $d = \\Tina4\\Database::create('x');\n    return \\Tina4\\Database::create('y');\n}\n";
        let (fm, _f, specs) = measured(Lang::Php, php, "App.php", false);
        assert!(specs.iter().any(|s| s.contains("DotEnv")), "got {specs:?}");
        assert!(specs.iter().any(|s| s.contains("Database")), "got {specs:?}");

        // Referencing Database TWICE is ONE dependency, not two - otherwise the
        // dashboard's dep_count badge inflates wildly on PHP's idiom.
        assert_eq!(fm.dep_count, 2, "distinct dependencies, not reference count: {specs:?}");

        let paths: HashSet<String> =
            ["DotEnv.php", "Database.php"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            resolve_import("\\Tina4\\DotEnv", "App.php", Lang::Php, &paths, Some("Tina4")),
            Some("DotEnv.php".to_string())
        );
    }

    #[test]
    fn php_use_statements_and_requires_are_extracted() {
        // The path in `use A\B;` lives one level down, inside a
        // namespace_use_clause. Matching only the declaration's direct children
        // silently found NOTHING, so PHP produced 0 edges over 138 real files.
        let php = "<?php\nnamespace Tina4;\nuse Tina4\\ORM;\nuse Tina4\\Database\\Database;\nrequire_once \"helpers.php\";\nfunction f($a) { return $a; }\n";
        let (fm, _f, specs) = measured(Lang::Php, php, "Frond.php", false);
        let mut got = specs.clone();
        got.sort();
        assert_eq!(
            got,
            vec![
                "Tina4\\Database\\Database".to_string(),
                "Tina4\\ORM".to_string(),
                "helpers.php".to_string(),
            ]
        );
        assert_eq!(fm.dep_count, 3);

        // ...and they resolve onto real files via PSR-4.
        let paths: HashSet<String> = ["ORM.php", "Database/Database.php", "helpers.php"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(
            resolve_import("Tina4\\ORM", "Frond.php", Lang::Php, &paths, Some("Tina4")),
            Some("ORM.php".to_string())
        );
        assert_eq!(
            resolve_import("Tina4\\Database\\Database", "Frond.php", Lang::Php, &paths, Some("Tina4")),
            Some("Database/Database.php".to_string())
        );
    }

    #[test]
    fn every_function_reports_its_own_loc() {
        // The dashboard's most-complex-functions table has a LOC column; without
        // this field every row rendered a literal "undefined".
        let py = "def small():\n    return 1\n\ndef bigger(a):\n    # comment line\n    if a:\n        return 2\n    return 3\n";
        let (_fm, fns) = analyze_py(py);
        let small = fns.iter().find(|f| f.name == "small").unwrap();
        let bigger = fns.iter().find(|f| f.name == "bigger").unwrap();
        assert_eq!(small.loc, 2, "def + return");
        // def + if + two returns = 4 code lines; the comment does NOT count,
        // matching the file-level is_code_line rule.
        assert_eq!(bigger.loc, 4, "comment excluded, same rule as file LOC");
        assert!(bigger.loc > small.loc);
        // and it is serialised, so the UI column can read it
        let json = serde_json::to_string(&bigger).unwrap();
        assert!(json.contains("\"loc\""), "loc must be in the JSON: {json}");
    }

    // ---- Generated-asset exclusion -------------------------------------------
    //
    // The engine is language-agnostic, so it sees `.js` the retired per-language
    // modules never did. One real minified bundle scored cyclomatic complexity
    // 26,416 on a single line and took the top FOUR offender slots, burying the
    // code a developer can actually fix. Build output is not source.

    #[test]
    fn minified_and_bundled_assets_are_excluded_by_name() {
        for n in [
            "tina4.min.js", "frond.min.js", "app.min.ts", "site.min.css",
            "vendor.bundle.js", "legacy-min.js", "app.js.map",
        ] {
            assert!(is_generated_asset(Path::new(n)), "{n} should be excluded");
        }
    }

    #[test]
    fn real_source_files_are_not_mistaken_for_assets() {
        for n in [
            "engine.py", "server.ts", "Frond.php", "cli.rb",
            "widget.js", "frond.js", "minify.js", "administer.ts",
        ] {
            assert!(!is_generated_asset(Path::new(n)), "{n} must still be analysed");
        }
    }

    #[test]
    fn a_bundle_not_named_like_one_is_caught_by_content() {
        // One enormous line is the minified shape, whatever the filename says.
        let minified = format!("var a=1;{}\n", "b(c,d);".repeat(400));
        assert!(looks_minified(&minified));

        // Ordinary source, even long, is not.
        let real = "def handler(request, response):\n    value = compute(request)\n    return response(value)\n".repeat(50);
        assert!(!looks_minified(&real), "normal source must not be skipped");

        // Degenerate inputs do not panic or false-positive.
        assert!(!looks_minified(""));
        assert!(!looks_minified("x = 1\n"));
    }

    // ---- Coupling resolution (the fix for the constant-1.0 instability) ------
    //
    // The retired per-framework implementation keyed its reverse graph on MODULE
    // NAMES while looking it up by FILE PATH, so afferent coupling was 0 for
    // every file, instability was the constant 1.0, and the dependency view drew
    // 0 edges from 902 recorded imports. These pin the corrected behaviour.

    fn pathset(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn python_relative_and_absolute_imports_resolve_to_files() {
        let paths = pathset(&["frond/engine.py", "frond/parser.py", "debug/__init__.py", "env.py"]);
        // `from .parser import x` inside frond/engine.py
        assert_eq!(
            resolve_import(".parser", "frond/engine.py", Lang::Python, &paths, Some("tina4_python")),
            Some("frond/parser.py".to_string())
        );
        // a package __init__ target
        assert_eq!(
            resolve_import("debug", "env.py", Lang::Python, &paths, Some("tina4_python")),
            Some("debug/__init__.py".to_string())
        );
        // absolute intra-package import while the scan root IS the package
        assert_eq!(
            resolve_import("tina4_python.env", "frond/engine.py", Lang::Python, &paths, Some("tina4_python")),
            Some("env.py".to_string())
        );
    }

    #[test]
    fn external_imports_are_not_internal_edges() {
        let paths = pathset(&["env.py", "app/main.ts"]);
        // stdlib / third-party must NOT become coupling
        assert_eq!(resolve_import("os", "env.py", Lang::Python, &paths, None), None);
        assert_eq!(resolve_import("json", "env.py", Lang::Python, &paths, None), None);
        // a bare TS specifier is a package, never a file in the tree
        assert_eq!(resolve_import("react", "app/main.ts", Lang::Ts, &paths, None), None);
        assert_eq!(resolve_import("node:fs", "app/main.ts", Lang::Ts, &paths, None), None);
    }

    #[test]
    fn typescript_relative_specifier_resolves_including_js_to_ts() {
        let paths = pathset(&["core/src/server.ts", "core/src/router.ts", "core/src/index.ts"]);
        assert_eq!(
            resolve_import("./router", "core/src/server.ts", Lang::Ts, &paths, None),
            Some("core/src/router.ts".to_string())
        );
        // TS writes ".js" in ESM but the file on disk is ".ts"
        assert_eq!(
            resolve_import("./router.js", "core/src/server.ts", Lang::Ts, &paths, None),
            Some("core/src/router.ts".to_string())
        );
        // a directory specifier picks up index.ts
        assert_eq!(
            resolve_import("../src", "core/other/x.ts", Lang::Ts, &paths, None),
            Some("core/src/index.ts".to_string())
        );
    }

    #[test]
    fn ruby_and_php_specifiers_resolve() {
        let rb = pathset(&["lib/tina4/frond.rb", "lib/tina4/orm.rb"]);
        assert_eq!(
            resolve_import("orm", "lib/tina4/frond.rb", Lang::Ruby, &rb, None),
            Some("lib/tina4/orm.rb".to_string())
        );
        assert_eq!(
            resolve_import("tina4/orm", "app.rb", Lang::Ruby, &rb, None),
            Some("lib/tina4/orm.rb".to_string())
        );
        let php = pathset(&["Tina4/Frond.php", "Tina4/ORM.php"]);
        assert_eq!(
            resolve_import("Tina4\\ORM", "Tina4/Frond.php", Lang::Php, &php, None),
            Some("Tina4/ORM.php".to_string())
        );
    }

    #[test]
    fn a_file_never_couples_to_itself() {
        let paths = pathset(&["a.py"]);
        // `import a` from inside a.py must not create a self-edge
        assert_eq!(resolve_import("a", "a.py", Lang::Python, &paths, None), None);
    }

    #[test]
    fn instability_is_a_real_spread_not_a_constant() {
        // Build a tiny real tree on disk and run the full two-pass analysis.
        let dir = std::env::temp_dir().join(format!("tina4_coupling_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // leaf.py is imported by two files and imports nothing -> ca=2, ce=0 -> 0.0
        fs::write(dir.join("leaf.py"), "def a():\n    return 1\n").unwrap();
        fs::write(dir.join("mid.py"), "import leaf\nimport os\ndef b():\n    return leaf.a()\n").unwrap();
        fs::write(dir.join("top.py"), "import leaf\nimport mid\ndef c():\n    return mid.b()\n").unwrap();

        let files = vec![dir.join("leaf.py"), dir.join("mid.py"), dir.join("top.py")];
        let report = analyze_targets(&files, dir.to_str().unwrap());
        let get = |n: &str| report.files.iter().find(|f| f.path == n).unwrap().clone();

        let leaf = get("leaf.py");
        assert_eq!(leaf.coupling_afferent, 2, "mid and top both import leaf");
        assert_eq!(leaf.coupling_efferent, 0);
        assert_eq!(leaf.instability, 0.0, "a pure dependency is maximally STABLE");

        let top = get("top.py");
        assert_eq!(top.coupling_afferent, 0, "nothing imports top");
        assert_eq!(top.coupling_efferent, 2, "top imports leaf and mid");
        assert_eq!(top.instability, 1.0, "a pure dependent is maximally UNSTABLE");

        let mid = get("mid.py");
        assert_eq!(mid.coupling_afferent, 1);
        assert_eq!(mid.coupling_efferent, 1, "os is external and excluded");
        assert_eq!(mid.instability, 0.5, "one in, one out");
        // dep_count keeps TOTAL-import semantics for the dashboard badge.
        assert_eq!(mid.dep_count, 2, "leaf + os");

        // Every recorded edge points at a file that actually exists.
        let known: HashSet<&String> = report.files.iter().map(|f| &f.path).collect();
        for (_from, targets) in report.dependency_graph.iter() {
            for t in targets {
                assert!(known.contains(t), "edge target {t} is not a scanned file");
            }
        }
        let total_edges: usize = report.dependency_graph.values().map(|v| v.len()).sum();
        assert_eq!(total_edges, 3, "leaf<-mid, leaf<-top, mid<-top");

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- Duplication / DRY ---------------------------------------------------
    //
    // Real files on a real filesystem, driven through the real `analyze_targets`
    // two-pass scan. Every case plants a KNOWN answer, so a detector that finds
    // nothing and a detector that flags everything both fail.

    /// Write `files` into a fresh temp dir and run the full scan over them.
    fn scan_temp(tag: &str, files: &[(&str, &str)]) -> (Report, PathBuf) {
        let dir = std::env::temp_dir()
            .join(format!("tina4_dup_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (name, body) in files {
            let p = dir.join(name);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, body).unwrap();
            paths.push(p);
        }
        let report = analyze_targets(&paths, dir.to_str().unwrap());
        (report, dir)
    }

    /// A block big enough to clear both gates, parameterised so callers can vary
    /// the identifiers (a Type-2 clone) or an operator (NOT a clone).
    fn dup_block(fn_name: &str, var: &str, op: &str) -> String {
        format!(
            "fn {fn_name}(input: i32) -> i32 {{
    let mut {var} = 0;
    for step in 0..input {{
        if step % 2 == 0 {{
            {var} = {var} {op} step;
        }} else if step % 3 == 0 {{
            {var} = {var} {op} (step * 2);
        }} else {{
            {var} = {var} {op} 1;
        }}
    }}
    if {var} > 100 {{
        {var} = 100;
    }}
    {var}
}}
"
        )
    }

    #[test]
    fn a_planted_duplicate_pair_is_found() {
        // THE positive gate. Two identical blocks, one file.
        let src = format!("{}\n{}", dup_block("alpha", "total", "+"), dup_block("beta", "total", "+"));
        let (report, dir) = scan_temp("pair", &[("a.rs", &src)]);
        assert!(
            !report.clones.is_empty(),
            "a planted identical pair must be reported as duplication"
        );
        let g = &report.clones[0];
        assert!(g.copies >= 2, "expected at least 2 copies, got {}", g.copies);
        assert!(
            report.offenders.iter().any(|o| o.kind == "duplication"),
            "the clone must surface as a `duplication` offender"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changing_one_of_the_pair_stops_it_being_reported() {
        // The other half of the same gate, and the one that proves the detector
        // is reading STRUCTURE rather than just finding any two big blocks.
        // `+` becomes `-` in one copy: same shape, different operator.
        let src = format!("{}\n{}", dup_block("alpha", "total", "+"), dup_block("beta", "total", "-"));
        let (report, dir) = scan_temp("broken", &[("a.rs", &src)]);
        assert!(
            report.clones.is_empty(),
            "changing an OPERATOR must break the match - shape hashing that ignored \
             operator tokens would still call these duplicates: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn renaming_identifiers_does_not_defeat_detection() {
        // Type-2 clone (Roy/Cordy): copy-paste-and-rename is the single most
        // common real duplication, and an engine that misses it is not measuring
        // DRY at all. Different function name, different variable name, identical
        // structure.
        let src = format!("{}\n{}", dup_block("alpha", "total", "+"), dup_block("gamma", "accumulator", "+"));
        let (report, dir) = scan_temp("renamed", &[("a.rs", &src)]);
        assert!(
            !report.clones.is_empty(),
            "a renamed copy is still a duplicate and must be found"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplication_is_detected_across_files() {
        // Cross-file is the whole point - a single-file duplication number is
        // nearly worthless, and it is also the case an accumulator scoped to one
        // file would silently miss.
        let a = dup_block("alpha", "total", "+");
        let b = dup_block("beta", "total", "+");
        let (report, dir) = scan_temp("xfile", &[("a.rs", &a), ("b.rs", &b)]);
        let cross: Vec<&CloneGroup> = report.clones.iter().filter(|c| c.cross_file).collect();
        assert!(
            !cross.is_empty(),
            "the same block in two different files must be reported as cross-file: {:?}",
            report.clones
        );
        assert_eq!(cross[0].files.len(), 2, "both files must be named");
        assert_eq!(cross[0].occurrences.len(), 2, "every clone occurrence must be named");
        assert!(cross[0].occurrences.iter().all(|o| o.start_line > 0 && o.end_line >= o.start_line));
        assert!(
            report
                .offenders
                .iter()
                .any(|o| o.kind == "duplication" && o.detail.contains("across 2 files")),
            "the offender detail must say it spans files"
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// NEGATIVE CONTROL. A detector that flags everything would pass every test
    /// above. This file has NO duplication and must produce none.
    #[test]
    fn duplication_negative_control_distinct_code_is_not_flagged() {
        let src = "\
fn parse_port(raw: &str) -> Option<u16> {
    raw.trim().parse::<u16>().ok()
}

fn banner(name: &str, version: &str) -> String {
    format!(\"{name} v{version} ready\")
}

fn is_even(n: i64) -> bool {
    n % 2 == 0
}

fn clamp_ratio(value: f64) -> f64 {
    if value < 0.0 {
        return 0.0;
    }
    if value > 1.0 {
        return 1.0;
    }
    value
}
";
        let (report, dir) = scan_temp("clean", &[("clean.rs", src)]);
        assert!(
            report.clones.is_empty(),
            "structurally distinct functions must NOT be reported as duplicates: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// The false-positive case that kills duplication tools in practice: a file
    /// full of near-identical trivial accessors. They ARE the same shape, and a
    /// size gate is the only thing standing between a useful report and one
    /// nobody reads.
    #[test]
    fn trivial_repeated_accessors_are_below_the_reporting_floor() {
        let src = "\
struct Config { host: String, port: String, user: String, pass: String, name: String }

impl Config {
    fn host(&self) -> &str { &self.host }
    fn port(&self) -> &str { &self.port }
    fn user(&self) -> &str { &self.user }
    fn pass(&self) -> &str { &self.pass }
    fn name(&self) -> &str { &self.name }
}
";
        let (report, dir) = scan_temp("getters", &[("cfg.rs", src)]);
        assert!(
            report.clones.is_empty(),
            "five identical one-line getters must NOT be reported - they are \
             identical by nature and unifying them would make the code worse: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplication_works_for_every_language_not_just_rust() {
        // Language-agnostic by construction: the detector never names a node
        // kind, so a sixth language would need no work here. Proven, not
        // asserted - each language gets its own planted pair.
        let py_block = |n: &str| format!(
            "def {n}(items):\n    total = 0\n    for item in items:\n        if item > 0:\n            total += item\n        elif item < 0:\n            total -= item\n        else:\n            total += 1\n    if total > 100:\n        total = 100\n    return total\n");
        let php_block = |n: &str| format!(
            "function {n}($items) {{\n    $total = 0;\n    foreach ($items as $item) {{\n        if ($item > 0) {{\n            $total += $item;\n        }} elseif ($item < 0) {{\n            $total -= $item;\n        }} else {{\n            $total += 1;\n        }}\n    }}\n    if ($total > 100) {{\n        $total = 100;\n    }}\n    return $total;\n}}\n");
        let rb_block = |n: &str| format!(
            "def {n}(items)\n  total = 0\n  items.each do |item|\n    if item > 0\n      total += item\n    elsif item < 0\n      total -= item\n    else\n      total += 1\n    end\n  end\n  if total > 100\n    total = 100\n  end\n  total\nend\n");
        let ts_block = |n: &str| format!(
            "export function {n}(items: number[]): number {{\n    let total = 0;\n    for (const item of items) {{\n        if (item > 0) {{\n            total += item;\n        }} else if (item < 0) {{\n            total -= item;\n        }} else {{\n            total += 1;\n        }}\n    }}\n    if (total > 100) {{\n        total = 100;\n    }}\n    return total;\n}}\n");

        for (tag, name, body) in [
            ("py", "a.py", format!("{}\n{}", py_block("alpha"), py_block("beta"))),
            ("php", "a.php", format!("<?php\n{}\n{}", php_block("alpha"), php_block("beta"))),
            ("rb", "a.rb", format!("{}\n{}", rb_block("alpha"), rb_block("beta"))),
            ("ts", "a.ts", format!("{}\n{}", ts_block("alpha"), ts_block("beta"))),
        ] {
            let (report, dir) = scan_temp(tag, &[(name, &body)]);
            assert!(
                !report.clones.is_empty(),
                "{tag}: a planted duplicate pair must be found in EVERY language, \
                 not just Rust"
            );
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn nested_copies_of_one_clone_are_reported_once() {
        // A duplicated block also duplicates every sub-block inside it, at every
        // wrapper level. Without maximal-only suppression the same clone lands in
        // the report three or four times and buries the real findings - which is
        // exactly what the first working version did (92 groups collapsed to 45).
        let src = format!("{}\n{}", dup_block("alpha", "total", "+"), dup_block("beta", "total", "+"));
        let (report, dir) = scan_temp("nested", &[("a.rs", &src)]);
        let at_same_place: Vec<&CloneGroup> = report
            .clones
            .iter()
            .filter(|c| c.first_file == "a.rs" && c.first_line == report.clones[0].first_line)
            .collect();
        assert_eq!(
            at_same_place.len(),
            1,
            "one duplicated region must yield ONE group, not one per nesting level: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);
    }

    // ---- Parse-health guard --------------------------------------------------
    //
    // tree-sitter always returns a tree, so a file it cannot parse still yields
    // LOC, CC and MI - all confidently wrong, because the decision points inside
    // an ERROR region are invisible to the walks that count them. Found via a
    // Delphi grammar gap that put 51.5% of a real corpus inside ERROR regions,
    // but it applies to every language the engine claims.

    /// A real, multi-branch function per language. Long enough to have
    /// decisions, functions and MI worth reporting, so "it still reports" means
    /// something.
    const HEALTHY: [(&str, &str, &str); 5] = [
        (
            "python", "ok.py",
            "def classify(value, limit):\n    if value is None:\n        return 'none'\n    if value > limit:\n        return 'high'\n    elif value < 0:\n        return 'negative'\n    return 'ok'\n\n\ndef total(rows):\n    out = 0\n    for row in rows:\n        if row:\n            out += row\n    return out\n",
        ),
        (
            "php", "ok.php",
            "<?php\nfunction classify($value, $limit) {\n    if ($value === null) { return 'none'; }\n    if ($value > $limit) { return 'high'; }\n    elseif ($value < 0) { return 'negative'; }\n    return 'ok';\n}\n\nfunction total($rows) {\n    $out = 0;\n    foreach ($rows as $row) {\n        if ($row) { $out += $row; }\n    }\n    return $out;\n}\n",
        ),
        (
            "ruby", "ok.rb",
            "def classify(value, limit)\n  return 'none' if value.nil?\n  return 'high' if value > limit\n  return 'negative' if value < 0\n  'ok'\nend\n\ndef total(rows)\n  out = 0\n  rows.each do |row|\n    out += row if row\n  end\n  out\nend\n",
        ),
        (
            "ts", "ok.ts",
            "export function classify(value: number | null, limit: number): string {\n  if (value === null) { return 'none'; }\n  if (value > limit) { return 'high'; }\n  else if (value < 0) { return 'negative'; }\n  return 'ok';\n}\n\nexport function total(rows: number[]): number {\n  let out = 0;\n  for (const row of rows) {\n    if (row) { out += row; }\n  }\n  return out;\n}\n",
        ),
        (
            "rust", "ok.rs",
            "pub fn classify(value: Option<i32>, limit: i32) -> &'static str {\n    let Some(v) = value else { return \"none\" };\n    if v > limit {\n        \"high\"\n    } else if v < 0 {\n        \"negative\"\n    } else {\n        \"ok\"\n    }\n}\n\npub fn total(rows: &[i32]) -> i32 {\n    let mut out = 0;\n    for row in rows {\n        if *row != 0 {\n            out += row;\n        }\n    }\n    out\n}\n",
        ),
    ];

    #[test]
    fn healthy_real_source_parses_at_full_health_in_every_language() {
        // The calibration behind MIN_PARSE_HEALTH, and the control for the
        // refusal tests below: if this drifts, the floor is wrong, not the guard.
        for (name, _file, src) in HEALTHY {
            let lang = match name {
                "python" => Lang::Python,
                "php" => Lang::Php,
                "ruby" => Lang::Ruby,
                "ts" => Lang::Ts,
                _ => Lang::Rust,
            };
            let (fm, _f, _s) = measured(lang, src, "t", false);
            assert_eq!(fm.parse_health, 1.0, "{name}: clean source must parse at 1.0");
            assert!(
                fm.parse_health >= MIN_PARSE_HEALTH,
                "{name}: healthy source must never be refused"
            );
        }
    }

    #[test]
    fn a_healthy_file_still_reports_full_metrics_in_every_language() {
        // THE NEGATIVE CONTROL, and the reason the refusal tests mean anything.
        // A guard that refused everything would pass every test above this one.
        // So: drive the same five languages through the WHOLE scan and demand
        // real numbers out the other side.
        for (name, file, src) in HEALTHY {
            let (report, dir) = scan_temp(&format!("healthy_{name}"), &[(file, src)]);
            assert_eq!(
                report.refused.len(),
                0,
                "{name}: healthy source must NOT be refused - {:?}",
                report.refused.iter().map(|r| &r.reason).collect::<Vec<_>>()
            );
            assert_eq!(report.files.len(), 1, "{name}: healthy source must be measured");
            let fm = &report.files[0];
            assert_eq!(fm.parse_health, 1.0, "{name}: health");
            let expected_functions = if name == "ruby" { 3 } else { 2 };
            assert_eq!(
                fm.functions,
                expected_functions,
                "{name}: named functions and nested callable blocks must be found"
            );
            assert!(fm.loc >= 10, "{name}: LOC must be real, got {}", fm.loc);
            assert!(fm.complexity >= 4, "{name}: CC must count the branches, got {}", fm.complexity);
            assert!(
                fm.maintainability > 0.0,
                "{name}: MI must be reported, got {}",
                fm.maintainability
            );
            assert!(
                !report.offenders.iter().any(|o| o.kind == "unparsed"),
                "{name}: healthy source must raise no `unparsed` offender"
            );
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_badly_broken_file_is_refused_in_every_language() {
        // Real garbage, not a mock: text that the real grammar genuinely cannot
        // parse. Each is structurally broken from near the top so the ERROR
        // region covers most of the file, which is exactly the Delphi shape.
        let broken = [
            ("python", "py", "def f(:\n  ??? not python at all ~~~\n  <<<>>>\n  ]]]}}}\n  def ((\n"),
            ("php", "php", "<?php\nfunction ((( {{{ ]]] ???\n  &&& ||| >>>\n  class class class\n"),
            ("ruby", "rb", "def f(\n  ??? ]]] }}}\n  end end end end\n  class class class\n"),
            ("ts", "ts", "function ((( {{{ ]]]\n  ??? >>> <<<\n  class class class\n  ))) }}}\n"),
            ("rust", "rs", "fn f( { ]]] ???\n  >>> <<< |||\n  struct struct struct\n  ))) }}}\n"),
        ];
        for (name, ext, src) in broken {
            let (report, dir) = scan_temp(
                &format!("broken_{name}"),
                &[(&format!("bad.{ext}"), src)],
            );
            assert_eq!(
                report.files.len(),
                0,
                "{name}: an unparseable file must NOT appear in file_metrics - \
                 its numbers would poison every average"
            );
            assert_eq!(
                report.refused.len(),
                1,
                "{name}: an unparseable file must be REFUSED, not silently skipped"
            );
            assert!(
                report.refused[0].parse_health < MIN_PARSE_HEALTH,
                "{name}: refused at health {}",
                report.refused[0].parse_health
            );
            // Visible in the ranked list, not only in a section nobody reads.
            assert!(
                report.offenders.iter().any(|o| o.kind == "unparsed"
                    && o.detail.contains("NOT MEASURED")),
                "{name}: refusal must surface as an `unparsed` offender"
            );
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_refused_file_does_not_drag_down_a_healthy_scan() {
        // The reason refusal beats "report it anyway": one unreadable file must
        // not move the numbers for the files that ARE readable.
        let good = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let (solo, d1) = scan_temp("solo_ok", &[("good.rs", good)]);
        let (mixed, d2) = scan_temp(
            "mixed_ok",
            &[("good.rs", good), ("bad.rs", "fn f( { ]]] ???\n  >>> <<< |||\n  struct struct struct\n  ))) }}}\n")],
        );
        assert_eq!(mixed.files.len(), 1, "only the healthy file is measured");
        assert_eq!(mixed.refused.len(), 1);
        assert_eq!(
            solo.files[0].maintainability, mixed.files[0].maintainability,
            "the healthy file's MI must be identical whether or not a broken \
             file sat next to it"
        );
        let _ = fs::remove_dir_all(d1);
        let _ = fs::remove_dir_all(d2);
    }

    #[test]
    fn a_refused_file_contributes_no_duplication() {
        // Shapes hashed out of a misparse are noise. Two identically-broken
        // files would otherwise "duplicate" each other and invent a finding.
        let junk = "fn f( { ]]] ???\n  >>> <<< |||\n  struct struct struct\n  ))) }}}\n  ??? ]]] {{{\n  <<< >>> |||\n  fn fn fn fn\n";
        let (report, dir) = scan_temp("junk_dup", &[("a.rs", junk), ("b.rs", junk)]);
        assert_eq!(report.refused.len(), 2, "both are rubble");
        assert!(
            report.clones.is_empty(),
            "two unparseable files must not be reported as duplicates of each \
             other: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn generated_doxygen_output_is_not_scanned_as_source() {
        // Scanning tina4delphi used to report metrics for seven Doxygen files
        // and nothing else - a report entirely about code nobody wrote.
        // Detected by Doxygen's own marker file, not by directory name, because
        // `docs/` and `html/` are legitimate places for hand-written source.
        let dir = std::env::temp_dir().join(format!("tina4_doxy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let gen = dir.join("docs/html");
        fs::create_dir_all(&gen).unwrap();
        fs::write(gen.join("doxygen.css"), "body{}\n").unwrap();
        fs::write(gen.join("search.js"), "function search(a){ if(a){return 1;} return 0; }\n").unwrap();
        // A hand-written source file in a plain docs dir must STILL be scanned.
        let real = dir.join("docs/examples");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("demo.js"), "function demo(a){ if(a){return 1;} return 0; }\n").unwrap();

        let mut found = Vec::new();
        walk_dir(&dir, &mut found, &[], true);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"search.js".to_string()),
            "generated Doxygen JS must not be scanned as source: {names:?}"
        );
        assert!(
            names.contains(&"demo.js".to_string()),
            "a real source file under docs/ must STILL be scanned - the marker \
             file is the signal, not the directory name: {names:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_refusal_is_loud_enough_to_fail_a_ci_gate() {
        // The whole point of refusing rather than skipping. A refused file has
        // to reach `--fail-on warn`, or a scan that measured nothing can still
        // exit 0 and read as green.
        let (report, dir) = scan_temp(
            "refusal_gate",
            &[("bad.py", "def f(:\n  ??? not python at all ~~~\n  <<<>>>\n  ]]]}}}\n  def ((\n")],
        );
        let summary = build_summary(&report, report.offenders.len());
        assert_eq!(summary.files_refused, 1, "the summary must count the refusal");
        assert_eq!(summary.files_analyzed, 0);
        let has_warn = report.offenders.iter().any(|o| o.severity == "warn");
        let has_error = report.offenders.iter().any(|o| o.severity == "error");
        assert!(has_warn, "a refusal must raise at least a warn");
        assert_eq!(
            compute_exit_code(Some("warn"), has_warn, has_error),
            1,
            "`--fail-on warn` must go red on a file the engine could not read"
        );
        // The other half, deliberately: refusal is a TOOLING gap, not a defect
        // in the code being measured, so it must not newly break existing
        // `--fail-on error` runs.
        assert_eq!(
            compute_exit_code(Some("error"), has_warn, has_error),
            0,
            "`--fail-on error` must stay green - a grammar gap is not the \
             author's error"
        );
        let _ = fs::remove_dir_all(dir);
    }

    // ---- Recursion depth (a PRE-EXISTING crash, not a duplication one) --------
    //
    // Five walks recurse per AST level. Reproduced against the 58f7f73 release
    // binary on macOS 26.5.2 arm64: a 60,000-term left-associative Python
    // expression, one term per line, aborts the process with
    // `fatal runtime error: stack overflow`, exit 134, taking the entire scan
    // with it. `looks_minified` does not fire - the file is 10 bytes per line
    // against its 200 threshold.

    /// A left-associative expression `terms` deep, one term per line, so
    /// `looks_minified` cannot short-circuit the case under test.
    fn deep_expression(terms: usize) -> String {
        let mut src = String::from("x = (a0\n");
        for i in 1..terms {
            src.push_str(&format!("  + a{i}\n"));
        }
        src.push_str(")\n");
        src
    }

    #[test]
    fn a_pathologically_deep_file_is_refused_instead_of_aborting_the_scan() {
        let src = deep_expression(MAX_AST_DEPTH + 200);
        assert!(
            !looks_minified(&src),
            "the fixture must reach the guard, not be filtered as a bundle"
        );
        let good = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let (report, dir) = scan_temp("deep", &[("deep.py", &src), ("ok.rs", good)]);
        // Reaching this line at all is most of the assertion: the pre-change
        // binary never returns from here.
        assert_eq!(report.refused.len(), 1, "the deep file must be refused");
        assert!(
            report.refused[0].reason.contains("nests deeper"),
            "the reason must name the real cause: {}",
            report.refused[0].reason
        );
        assert_eq!(
            report.files.len(),
            1,
            "the REST of the scan must survive - one bad file must not cost the \
             whole report"
        );
        assert_eq!(report.files[0].path, "ok.rs");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deeply_nested_but_survivable_file_is_still_measured() {
        // The negative control for the depth guard. A limit that refused
        // anything nested at all would pass the test above and be useless.
        // 600 is 7x deeper than the deepest real file in the whole corpus
        // (79, src/agent.rs) and still comfortably measured.
        let src = deep_expression(600);
        let (report, dir) = scan_temp("deep_ok", &[("deepish.py", &src)]);
        assert_eq!(
            report.refused.len(),
            0,
            "600 levels is under the {MAX_AST_DEPTH} limit and must be measured: {:?}",
            report.refused.iter().map(|r| &r.reason).collect::<Vec<_>>()
        );
        assert_eq!(report.files.len(), 1);
        assert!(report.files[0].loc > 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn depth_guard_boundary_is_exact() {
        // Pinned on a REAL parsed tree, both sides, so the guard cannot drift
        // off by one in either direction.
        let mut parser = Parser::new();
        parser.set_language(&Lang::Python.tree_sitter_language()).unwrap();
        let tree = parser.parse(deep_expression(50), None).unwrap();
        let root = tree.root_node();
        // Measure the tree's true depth iteratively, then bracket it.
        let mut depth = 0usize;
        let mut stack = vec![(root, 0usize)];
        while let Some((node, d)) = stack.pop() {
            if d > depth {
                depth = d;
            }
            let mut c = node.walk();
            for child in node.children(&mut c) {
                stack.push((child, d + 1));
            }
        }
        assert!(depth > 10, "the fixture must actually be deep, got {depth}");
        assert!(!depth_exceeds(root, depth), "depth {depth} does not EXCEED {depth}");
        assert!(
            depth_exceeds(root, depth - 1),
            "depth {depth} does exceed {}",
            depth - 1
        );
    }

    // ---- Duplication must never be built out of a misparse -------------------

    #[test]
    fn an_error_region_never_becomes_a_clone_fragment() {
        // Seven lines of garbage that tree-sitter folds into ONE ERROR node.
        // Measured on tree-sitter 0.26.11: that node is 69 nodes over 7 lines,
        // so it clears MIN_CLONE_NODES (60) and MIN_CLONE_LINES (6) on its own
        // and the pre-change `collect_fragments` emitted it as a candidate.
        let junk = "class ][ oops @@@\n  %%%% ????\n  ]]] [[[ }}}\n  &&& ||| ^^^\n  ~~~ !!! @@@\n  ((( ))) {{{\n  ,,, ... ;;;\n";
        let mut parser = Parser::new();
        parser.set_language(&Lang::Python.tree_sitter_language()).unwrap();
        let tree = parser.parse(junk, None).unwrap();

        // First prove the fixture is still the hard case it was written to be:
        // an ERROR node that WOULD qualify if nothing stopped it.
        let mut qualifying_error = false;
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let mut c = node.walk();
            let children: Vec<Node> = node.children(&mut c).collect();
            if node.is_error() {
                let mut count = 0u32;
                let mut inner = vec![node];
                while let Some(n) = inner.pop() {
                    count += 1;
                    let mut ic = n.walk();
                    for ch in n.children(&mut ic) {
                        inner.push(ch);
                    }
                }
                let lines = node.end_position().row - node.start_position().row + 1;
                if count >= MIN_CLONE_NODES && lines >= MIN_CLONE_LINES {
                    qualifying_error = true;
                }
            }
            stack.extend(children);
        }
        assert!(
            qualifying_error,
            "fixture no longer contains an ERROR node big enough to clear both \
             clone gates - it is not testing anything"
        );

        let mut fragments: Vec<CloneFragment> = Vec::new();
        let (_hash, _count, clean) = collect_fragments(tree.root_node(), 0, junk.as_bytes(), &mut fragments);
        assert!(!clean, "a tree with an ERROR node is not clean");
        assert!(
            fragments.is_empty(),
            "no fragment may be hashed out of a parse error: {fragments:?}"
        );
    }

    #[test]
    fn two_differently_broken_files_are_never_duplicates_of_each_other() {
        // The failure this guards against, stated plainly: being unparseable is
        // not a similarity. Two files whose only common property is that the
        // grammar gave up on them must produce no finding at all.
        let a = "def f(:\n  ??? not python ~~~\n  <<<>>>\n  ]]]}}}\n  def ((\n  class ][\n  @@@ %%%\n";
        let b = "class ][ oops @@@\n  %%%% ????\n  ]]] [[[ }}}\n  &&& ||| ^^^\n  ~~~ !!! @@@\n  ((( ))) {{{\n  ,,, ... ;;;\n";
        let (report, dir) = scan_temp("two_broken", &[("a.py", a), ("b.py", b)]);
        assert_eq!(report.refused.len(), 2, "both are rubble");
        assert!(
            report.clones.is_empty(),
            "two DIFFERENT unparseable files must not be reported as duplicates: {:?}",
            report.clones
        );
        assert!(
            !report.offenders.iter().any(|o| o.kind == "duplication"),
            "no duplication offender may come out of two misparses"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn different_string_bodies_are_not_a_clone() {
        // Two template literals with the SAME `${...}` interpolation skeleton but
        // ENTIRELY different text must not be reported as duplicates. Before the
        // string-leaf fold this collided - the structural hash saw only the shape
        // and discarded the body - which is the mongoClient.ts / syncBridge.ts
        // phantom "229 duplicated lines" false positive.
        let mk = |word: &str| -> String {
            let mut s = String::from("const S = { a: 1 };\nexport const T = `");
            for _ in 0..30 {
                s.push_str(word);
                s.push_str(" ${S.a}\n");
            }
            s.push_str("`;\n");
            s
        };
        let a = mk("alphaworker");
        let b = mk("bravohelper");
        let (report, dir) = scan_temp("dup_strbody", &[("a.ts", a.as_str()), ("b.ts", b.as_str())]);
        assert!(
            !report.offenders.iter().any(|o| o.kind == "duplication"),
            "unrelated string bodies that share an interpolation shape are not a clone: {:?}",
            report.clones
        );
        let _ = fs::remove_dir_all(dir);

        // Positive control: IDENTICAL template literals are STILL a clone - the fix
        // makes the detector more precise, it does not switch duplication off.
        let same = mk("sameworker");
        let (report2, dir2) =
            scan_temp("dup_strsame", &[("a.ts", same.as_str()), ("b.ts", same.as_str())]);
        assert!(
            report2.offenders.iter().any(|o| o.kind == "duplication"),
            "identical template literals must still be detected as a clone"
        );
        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn comments_are_ignored_for_type_2_duplication() {
        // Roy/Cordy Type-II tolerates comments. Documentation strings remain
        // executable string expressions and are deliberately not comments.
        //
        // The positive half is the same test: identifiers and same-kind literals
        // ARE normalised away, which is what makes the detector worth having.
        fn shape(lang: Lang, src: &str) -> u64 {
            let mut parser = Parser::new();
            parser.set_language(&lang.tree_sitter_language()).unwrap();
            let tree = parser.parse(src, None).unwrap();
            let mut out = Vec::new();
            collect_fragments(tree.root_node(), 0, src.as_bytes(), &mut out).0
        }
        // Renaming is invisible - the property the whole metric rests on.
        assert_eq!(
            shape(Lang::Python, "def a(x):\n    return x + 1\n"),
            shape(Lang::Python, "def b(y):\n    return y + 1\n"),
            "an identifier rename must NOT change the shape"
        );
        // A same-kind literal change is invisible too.
        assert_eq!(
            shape(Lang::Python, "def a(x):\n    return x + 1\n"),
            shape(Lang::Python, "def a(x):\n    return x + 2\n"),
            "a same-kind literal change must NOT change the shape"
        );
        // Layout is invisible - whitespace never becomes a node.
        assert_eq!(
            shape(Lang::Python, "def a(x):\n    return x + 1\n"),
            shape(Lang::Python, "def a(x):\n        return  x  +  1\n"),
            "reformatting must NOT change the shape"
        );
        // An operator IS significant - the anonymous token is hashed.
        assert_ne!(
            shape(Lang::Python, "def a(x):\n    return x + 1\n"),
            shape(Lang::Python, "def a(x):\n    return x - 1\n"),
            "`+` and `-` must not collide"
        );
        // Comments are normalised away in every supported language.
        for (name, lang, plain, commented) in [
            (
                "python", Lang::Python,
                "def a(x):\n    return x + 1\n",
                "def a(x):\n    # explain\n    return x + 1\n",
            ),
            (
                "php", Lang::Php,
                "<?php\nfunction a($x) { return $x + 1; }\n",
                "<?php\n/** d */\nfunction a($x) { return $x + 1; }\n",
            ),
            (
                "ruby", Lang::Ruby,
                "def a(x)\n  x + 1\nend\n",
                "# c\ndef a(x)\n  x + 1\nend\n",
            ),
            (
                "ts", Lang::Ts,
                "function a(x: number) { return x + 1; }\n",
                "// c\nfunction a(x: number) { return x + 1; }\n",
            ),
            (
                "rust", Lang::Rust,
                "fn a(x: i32) -> i32 { x + 1 }\n",
                "/// doc\nfn a(x: i32) -> i32 { x + 1 }\n",
            ),
        ] {
            assert_eq!(
                shape(lang, plain),
                shape(lang, commented),
                "{name}: comments must not defeat a Type-2 clone"
            );
        }
        assert_ne!(
            shape(Lang::Python, "def a(x):\n    return x + 1\n"),
            shape(Lang::Python, "def a(x):\n    \"runtime value\"\n    return x + 1\n"),
            "a Python string expression is executable syntax, not a comment"
        );
    }

    // ---- Stable formula calibration -----------------------------------------

    /// The Python implementation was retired by ADR-0054 and now delegates back
    /// to this binary, so invoking it here would compare the engine with itself
    /// (or, worse, with an older installed binary). Lock the accepted baseline
    /// directly; callable allocation is covered by the cross-language test.
    #[test]
    fn scope_neutral_formula_fixture_stays_stable() {
        let src = read_fixture("sample_container.py");
        let (fm, _) = analyze_py(&src);

        assert_eq!(fm.loc, 81);
        assert_eq!(fm.complexity, 14);
        assert_eq!(fm.functions, 7);
        assert_eq!(fm.maintainability, 39.4);
        assert_eq!(fm.avg_complexity, 2.0);
    }

    // ── Test detection: the class-symbol stage ──────────────────────────
    //
    // A test that imports through the package root never mentions the module's
    // file stem, so stages 1 and 2 both miss it. Before this stage existed,
    // `tina4 metrics` reported a well-tested module as untested and raised an
    // "untested" offender for it.

    #[test]
    fn declared_reference_names_finds_a_short_class() {
        let names = declared_reference_names("class ORM:\n    def save(self):\n        return True\n", Lang::Python);
        assert!(names.contains(&"ORM".to_string()), "got {names:?}");
    }

    #[test]
    fn a_three_char_class_referenced_by_a_test_counts_as_tested() {
        // No length floor. A >3-char gate was the bug the Python master fixed:
        // it excluded exactly the short framework types that matter (ORM, Api, Log).
        let idx = test_index(
            "test_models.py",
            "from src import ORM\n\ndef test_save():\n    assert ORM().save()\n",
        );
        let declared = vec!["ORM".to_string()];
        assert!(
            module_has_tests(Path::new("src/orm.py"), &idx, &declared),
            "the ORM class symbol is the only signal here and it is a real one"
        );
    }

    #[test]
    fn an_unreferenced_class_is_still_untested() {
        // The negative half. Without it this stage could return true for
        // anything and the test above would still pass.
        let idx = test_index("test_other.py", "def test_nothing():\n    assert True\n");
        let declared = vec!["Widget".to_string()];
        assert!(
            !module_has_tests(Path::new("src/widget.py"), &idx, &declared),
            "a class no test mentions must not be reported as tested"
        );
    }

    #[test]
    fn a_symbol_match_is_whole_identifier_only() {
        // Substring matching would let Order mark OrderItem as tested, and every
        // short name would collide constantly. This is what makes dropping the
        // length floor safe.
        assert!(mentions_symbol("assert ORM().save()", "ORM"));
        assert!(mentions_symbol("from src import ORM", "ORM"));
        assert!(!mentions_symbol("class ORMBase: pass", "ORM"));
        assert!(!mentions_symbol("x = MyORM()", "ORM"));
        assert!(!mentions_symbol("FORMAT = 1", "ORM"));
        assert!(mentions_symbol("Order(1)", "Order"));
        assert!(!mentions_symbol("OrderItem(1)", "Order"));
    }

    // ── PHPUnit's PascalCase test filename (Metrics.php -> MetricsTest.php) ──
    // Every other stage-1 pattern uses a separator, so this convention matched
    // nothing and EVERY PHP source file raised a false "untested" offender.

    #[test]
    fn a_typescript_interface_is_a_declared_type() {
        // An interface-only module has no class, so without this the module was
        // reported UNTESTED however plainly a test referenced it.
        let names = declared_reference_names(
            "export interface WidgetConnection {\n  id: string;\n}\n", Lang::Ts);
        assert!(names.contains(&"WidgetConnection".to_string()), "got {names:?}");
    }

    #[test]
    fn a_typescript_interface_referenced_by_a_test_counts_as_tested() {
        // The TS inline-type-import idiom: the reference sits mid-line on a
        // `const` declaration, so no import-line rule can see it. The type NAME
        // is the only signal, which is exactly what stage 3 is for.
        let idx = test_index(
            "widget.test.ts",
            "const c: import(\"../src/widgetConnection.ts\").WidgetConnection = { id: \"1\" };",
        );
        let declared = vec!["WidgetConnection".to_string()];
        assert!(module_has_tests(Path::new("src/widgetConnection.ts"), &idx, &declared));
    }

    #[test]
    fn an_unreferenced_interface_is_still_untested() {
        let idx = test_index("other.test.ts", "const x = 1;");
        let declared = vec!["SoloIface".to_string()];
        assert!(!module_has_tests(Path::new("src/ctrlIface.ts"), &idx, &declared));
    }

    #[test]
    fn a_phpunit_pascalcase_test_file_counts_as_tested() {
        let idx = test_index("MetricsTest.php", "<?php\nclass MetricsTest {}\n");
        assert!(
            module_has_tests(Path::new("Tina4/Metrics.php"), &idx, &[]),
            "MetricsTest.php is the dedicated test for Metrics.php"
        );
    }

    #[test]
    fn a_pascalcase_match_is_anchored_not_a_substring() {
        // The negative half: `Base` must NOT be marked tested by DatabaseTest.php.
        // starts_with (not contains) is what makes the separator-less pattern safe.
        let idx = test_index("DatabaseTest.php", "<?php\nclass DatabaseTest {}\n");
        assert!(
            !module_has_tests(Path::new("Tina4/Base.php"), &idx, &[]),
            "DatabaseTest.php tests Database, not Base"
        );
    }

    #[test]
    fn multiline_and_dynamic_typescript_imports_are_test_references() {
        let multiline = test_index(
            "storage.test.ts",
            "import {\n  persist,\n} from '../src/storage/persist';\ntest('persist', () => persist('x'));",
        );
        assert!(module_has_tests(Path::new("src/storage/persist.ts"), &multiline, &[]));

        let dynamic = test_index(
            "edge-cases.test.ts",
            "test('loads tracker', async () => { await import('../src/debug/tracker'); });",
        );
        assert!(module_has_tests(Path::new("src/debug/tracker.ts"), &dynamic, &[]));
    }

    #[test]
    fn a_barrel_import_can_reference_an_exported_function() {
        let declared = declared_reference_names(
            "export function persist(value: string) { return value; }\n",
            Lang::Ts,
        );
        assert!(declared.contains(&"persist".to_string()));
        let index = test_index(
            "storage.test.ts",
            "import { persist } from '../src';\ntest('value', () => persist('x'));",
        );
        assert!(module_has_tests(Path::new("src/storage/persist.ts"), &index, &declared));
    }

    #[test]
    fn a_shared_ruby_namespace_is_not_a_test_reference() {
        let declared = declared_reference_names("module Tina4\n  VERSION = '1'\nend\n", Lang::Ruby);
        assert!(!declared.contains(&"Tina4".to_string()), "a namespace is not an exported subject under test");
        let index = test_index("version_spec.rb", "expect(Tina4::VERSION).not_to be_nil");
        assert!(!module_has_tests(Path::new("lib/tina4/unrelated.rb"), &index, &declared));
    }

    #[test]
    fn production_source_defaults_and_globs_are_explicit() {
        for path in [
            "tests/widget.test.ts", "src/widget.spec.ts", "src/test_widget.py",
            "spec/widget_spec.rb", "Tina4/WidgetTest.php", "src/types.d.ts",
        ] {
            assert!(is_default_non_production_source(Path::new(path)), "{path} must not affect a production score");
        }
        assert!(!is_default_non_production_source(Path::new("src/widget.ts")));
        assert!(path_matches_glob("packages/core/src/dev/admin.ts", "**/dev/**"));
        assert!(path_matches_glob("src/generated/client.ts", "src/generated/*.ts"));
        assert!(!path_matches_glob("src/core/client.ts", "src/generated/*.ts"));
    }

    #[test]
    fn target_resolution_applies_repeatable_exclusions_and_allows_an_override() {
        let directory = std::env::temp_dir().join(format!(
            "tina4_metrics_exclusions_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        for relative in [
            "src/app.ts",
            "src/app.test.ts",
            "src/types.d.ts",
            "src/generated/client.ts",
            "src/dev/panel.ts",
        ] {
            let path = directory.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "export const value = 1;\n").unwrap();
        }
        let exclusions = vec!["**/generated/**".to_string(), "**/dev/**".to_string()];
        let (production, _) = resolve_targets(
            Some(directory.join("src").to_str().unwrap()),
            &exclusions,
            false,
        )
        .unwrap();
        assert_eq!(
            production.iter().map(|path| path.file_name().unwrap().to_string_lossy()).collect::<Vec<_>>(),
            vec!["app.ts"],
            "defaults and both explicit exclusions must compose"
        );
        let (with_non_production, _) = resolve_targets(
            Some(directory.join("src").to_str().unwrap()),
            &exclusions,
            true,
        )
        .unwrap();
        assert_eq!(with_non_production.len(), 3, "the override restores test and declaration source only");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn target_resolution_accepts_any_supported_source_directory() {
        let directory = std::env::temp_dir().join(format!(
            "tina4_metrics_arbitrary_source_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("widget.py"), "def widget():\n    return 1\n").unwrap();
        fs::write(directory.join("component.ts"), "export function component() { return 1; }\n").unwrap();
        fs::write(directory.join("notes.txt"), "not source\n").unwrap();

        let (files, root) = resolve_targets(Some(directory.to_str().unwrap()), &[], false).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(root, directory.to_string_lossy());
        assert_eq!(names, vec!["component.ts", "widget.py"]);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn json_names_the_test_signal_honestly() {
        let (metrics, _functions) = analyze_py("def widget():\n    return 1\n");
        let value = serde_json::to_value(metrics).unwrap();
        assert_eq!(value["has_referencing_test"], false);
        assert!(value.get("has_tests").is_none(), "the old field overclaimed coverage");
    }

    // ── Run-history diffing ─────────────────────────────────────────────────
    fn snap(off: usize, dup: usize, maint: f64, cpx: f64) -> MetricsSnapshot {
        MetricsSnapshot {
            at: 1_000,
            tool_version: "test".to_string(),
            files_analyzed: 1,
            total_functions: 1,
            avg_complexity: cpx,
            avg_maintainability: maint,
            total_offenders: off,
            duplicate_blocks: 0,
            duplicate_lines: dup,
        }
    }
    fn hfile(off: usize, cc: u32) -> HistoryFile {
        HistoryFile { offenders: off, worst_cc: cc, maintainability: 50.0, loc: 100, ..HistoryFile::default() }
    }
    fn record(files: BTreeMap<String, HistoryFile>, last: MetricsSnapshot) -> HistoryRecord {
        HistoryRecord { schema: HISTORY_SCHEMA, scan_root: ".".to_string(), last, files, trend: vec![], runs: vec![] }
    }

    #[test]
    fn history_delta_flags_offender_improvement_and_regression() {
        let mut prev_files = BTreeMap::new();
        prev_files.insert("a.py".to_string(), hfile(3, 20));
        prev_files.insert("b.py".to_string(), hfile(1, 10));
        let prev = record(prev_files, snap(4, 0, 50.0, 3.0));
        let mut cur = BTreeMap::new();
        cur.insert("a.py".to_string(), hfile(1, 20)); // 3 -> 1  improved
        cur.insert("b.py".to_string(), hfile(2, 10)); // 1 -> 2  regressed
        let d = compute_delta(&prev, &snap(3, 0, 51.0, 2.9), &cur);
        assert!(d.improved_files.iter().any(|f| f.path == "a.py" && f.status == "improved"));
        assert!(d.regressed_files.iter().any(|f| f.path == "b.py" && f.status == "regressed"));
    }

    #[test]
    fn history_records_cc_wobble_on_a_clean_file() {
        // A file with zero offenders whose worst complexity rises is not an
        // improvement/regression gate, but the full history still records it.
        let mut prev_files = BTreeMap::new();
        prev_files.insert("a.py".to_string(), hfile(0, 3));
        let prev = record(prev_files, snap(0, 0, 80.0, 1.0));
        let mut cur = BTreeMap::new();
        cur.insert("a.py".to_string(), hfile(0, 9));
        let d = compute_delta(&prev, &snap(0, 0, 60.0, 5.0), &cur);
        assert!(
            d.improved_files.is_empty() && d.regressed_files.is_empty(),
            "cc wobble on a clean file is not an improvement/regression gate"
        );
        assert_eq!(d.changed_files.len(), 1);
    }

    #[test]
    fn history_reports_full_metric_changes_for_a_clean_file() {
        let mut prev_files = BTreeMap::new();
        prev_files.insert("a.py".to_string(), hfile(0, 3));
        let prev = record(prev_files, snap(0, 0, 80.0, 1.0));
        let mut after = hfile(0, 3);
        after.loc = 120;
        after.functions = 4;
        let mut cur = BTreeMap::new();
        cur.insert("a.py".to_string(), after);
        let d = compute_delta(&prev, &snap(0, 0, 80.0, 1.0), &cur);
        let file = d.changed_files.iter().find(|f| f.path == "a.py").expect("metric change should be visible");
        assert_eq!(file.status, "changed");
        assert_eq!(file.metrics_before.as_ref().unwrap().loc, 100);
        assert_eq!(file.metrics_after.as_ref().unwrap().loc, 120);
    }

    #[test]
    fn history_flags_cc_regression_on_an_offending_file() {
        let mut prev_files = BTreeMap::new();
        prev_files.insert("a.py".to_string(), hfile(1, 40));
        let prev = record(prev_files, snap(1, 0, 40.0, 4.0));
        let mut cur = BTreeMap::new();
        cur.insert("a.py".to_string(), hfile(1, 70)); // same offenders, worse cc
        let d = compute_delta(&prev, &snap(1, 0, 38.0, 4.5), &cur);
        let f = d
            .regressed_files
            .iter()
            .find(|f| f.path == "a.py")
            .expect("an offending file whose worst cc rose must regress");
        assert_eq!((f.worst_cc_before, f.worst_cc_after), (40, 70));
    }

    #[test]
    fn history_resolved_when_offending_file_disappears() {
        let mut prev_files = BTreeMap::new();
        prev_files.insert("gone.py".to_string(), hfile(2, 30));
        let prev = record(prev_files, snap(2, 0, 40.0, 3.0));
        let d = compute_delta(&prev, &snap(0, 0, 45.0, 2.0), &BTreeMap::new());
        assert!(d.improved_files.iter().any(|f| f.path == "gone.py" && f.status == "resolved"));
    }

    #[test]
    fn history_roundtrips_and_first_run_is_empty() {
        let dir = std::env::temp_dir().join(format!("tina4-hist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        assert!(load_history(&root).is_none(), "no baseline before the first save");
        let mut files = BTreeMap::new();
        files.insert("a.py".to_string(), hfile(1, 12));
        save_history(&root, None, snap(1, 5, 50.0, 3.0), files);
        let loaded = load_history(&root).expect("baseline should load back");
        assert_eq!(loaded.schema, HISTORY_SCHEMA);
        assert_eq!(loaded.last.total_offenders, 1);
        assert_eq!(loaded.files.get("a.py").map(|f| f.offenders), Some(1));
        assert_eq!(loaded.runs.len(), 1);
        assert_eq!(loaded.runs[0].files.get("a.py").map(|f| f.loc), Some(100));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_trend_accumulates_and_caps() {
        let dir = std::env::temp_dir().join(format!("tina4-trend-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().to_string();
        save_history(&root, None, snap(10, 0, 40.0, 3.0), BTreeMap::new());
        for i in 0..(TREND_CAP + 5) {
            let prev = load_history(&root);
            save_history(&root, prev, snap(i, 0, 40.0, 3.0), BTreeMap::new());
        }
        let loaded = load_history(&root).unwrap();
        assert!(
            loaded.trend.len() <= TREND_CAP,
            "trend capped at {TREND_CAP}, got {}",
            loaded.trend.len()
        );
        assert!(loaded.runs.len() <= TREND_CAP, "runs capped at {TREND_CAP}, got {}", loaded.runs.len());
        let _ = fs::remove_dir_all(&dir);
    }
}
