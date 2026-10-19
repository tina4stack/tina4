// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Run-history .tina4-metrics.json read/write + the regression gate. Split out of the metrics engine (ADR-0002); behaviour unchanged.

use super::*;

// ── Run history: regression / improvement tracking ──────────────────────────
// Every scan records a summary plus full per-file snapshots to
// `.tina4-metrics.json` in the scan root, so the NEXT run of the same scope can
// say what improved and what got worse. Zero new dependencies: the record is serde_json and timestamps are
// UNIX seconds from SystemTime. `--no-history` reads and writes nothing.
//
// The file is data, not source (a `.json`), so the engine never scans it and it
// never pollutes its own numbers. Commit it to gate regressions in CI, or add it
// to .gitignore for a purely local baseline - both work.

pub(crate) const HISTORY_FILE: &str = ".tina4-metrics.json";
pub(crate) const HISTORY_SCHEMA: u32 = 2;
pub(crate) const TREND_CAP: usize = 20;

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct MetricsSnapshot {
    /// UNIX seconds when this scan ran.
    pub(crate) at: u64,
    pub(crate) tool_version: String,
    pub(crate) files_analyzed: usize,
    pub(crate) total_functions: usize,
    pub(crate) avg_complexity: f64,
    pub(crate) avg_maintainability: f64,
    pub(crate) total_offenders: usize,
    pub(crate) duplicate_blocks: usize,
    pub(crate) duplicate_lines: usize,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub(crate) struct HistoryFile {
    pub(crate) offenders: usize,
    /// Highest cyclomatic complexity of any function in the file.
    pub(crate) worst_cc: u32,
    pub(crate) maintainability: f64,
    pub(crate) loc: usize,
    pub(crate) complexity: u32,
    pub(crate) avg_complexity: f64,
    pub(crate) functions: usize,
    pub(crate) halstead_volume: f64,
    pub(crate) has_referencing_test: bool,
    pub(crate) dep_count: usize,
    pub(crate) coupling_efferent: usize,
    pub(crate) coupling_afferent: usize,
    pub(crate) instability: f64,
    pub(crate) parse_health: f64,
    pub(crate) duplicate_groups: usize,
    pub(crate) duplicate_lines: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct HistoryRun {
    pub(crate) snapshot: MetricsSnapshot,
    pub(crate) files: BTreeMap<String, HistoryFile>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct HistoryRecord {
    pub(crate) schema: u32,
    pub(crate) scan_root: String,
    pub(crate) last: MetricsSnapshot,
    pub(crate) files: BTreeMap<String, HistoryFile>,
    /// Older summaries, oldest first, excluding `last`. Capped at TREND_CAP so
    /// the file cannot grow without bound.
    #[serde(default)]
    pub(crate) trend: Vec<MetricsSnapshot>,
    /// Complete per-file snapshots for each retained run, oldest first. This
    /// is the commit-friendly changelog; `last` and `files` remain as a compact
    /// current-run mirror for consumers of schema 1.
    #[serde(default)]
    pub(crate) runs: Vec<HistoryRun>,
}

/// One file's movement between two runs.
#[derive(Serialize)]
pub(crate) struct FileDelta {
    pub(crate) path: String,
    pub(crate) offenders_before: i64,
    pub(crate) offenders_after: i64,
    pub(crate) worst_cc_before: i64,
    pub(crate) worst_cc_after: i64,
    /// Full metric snapshots for this file, so a changelog consumer can see
    /// what changed without reopening the history file and joining paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metrics_before: Option<HistoryFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metrics_after: Option<HistoryFile>,
    /// "improved" | "regressed" | "new" | "resolved".
    pub(crate) status: String,
}

/// What changed since the previous recorded run.
#[derive(Serialize)]
pub(crate) struct RunDelta {
    /// Age of the previous run in seconds (how long since the baseline).
    pub(crate) since_secs: u64,
    pub(crate) before: MetricsSnapshot,
    pub(crate) after: MetricsSnapshot,
    pub(crate) improved_files: Vec<FileDelta>,
    pub(crate) regressed_files: Vec<FileDelta>,
    /// Files whose metrics changed without crossing the offender/complexity
    /// improvement gates. This makes clean-file churn visible as well.
    pub(crate) changed_files: Vec<FileDelta>,
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn history_path(scan_root: &str) -> PathBuf {
    Path::new(scan_root).join(HISTORY_FILE)
}

pub(crate) fn load_history(scan_root: &str) -> Option<HistoryRecord> {
    let raw = fs::read_to_string(history_path(scan_root)).ok()?;
    let rec: HistoryRecord = serde_json::from_str(&raw).ok()?;
    // A record from an incompatible schema is ignored (treated as no baseline)
    // rather than crashing a scan; the next save rewrites it at the current schema.
    if rec.schema != HISTORY_SCHEMA {
        return None;
    }
    Some(rec)
}

pub(crate) fn snapshot_of(summary: &Summary) -> MetricsSnapshot {
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
pub(crate) fn per_file_history(report: &Report) -> BTreeMap<String, HistoryFile> {
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

pub(crate) fn history_file_changed(before: &HistoryFile, after: &HistoryFile) -> bool {
    before != after
}

pub(crate) fn compute_delta(
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

pub(crate) fn save_history(
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
pub(crate) fn print_delta_human(delta: &RunDelta, scan_root: &str) {
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
pub(crate) fn human_age(secs: u64) -> String {
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

pub(crate) fn print_human(summary: &Summary, shown: &[Offender], suggest: bool) {
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
