// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Report summary + the human and JSON renderers. Split out of the metrics engine (ADR-0002); behaviour unchanged.

use super::*;

// ── Summary + JSON payload ────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct Summary {
    pub(crate) files_analyzed: usize,
    pub(crate) total_functions: usize,
    pub(crate) avg_complexity: f64,
    pub(crate) avg_maintainability: f64,
    pub(crate) scan_mode: String,
    pub(crate) scan_root: String,
    pub(crate) total_offenders: usize,
    /// Maximal duplicated blocks found across the whole scan, and the lines that
    /// would disappear if each group were unified to a single definition. Whole-
    /// scan facts, so they live on the summary rather than on any one file.
    pub(crate) duplicate_blocks: usize,
    pub(crate) duplicate_lines: usize,
    /// Files the parse-health guard REFUSED. Reported so a hole in the coverage
    /// is never invisible - the whole point of the guard.
    pub(crate) files_refused: usize,
}

#[derive(Serialize)]
pub(crate) struct JsonPayload {
    pub(crate) summary: Summary,
    pub(crate) offenders: Vec<Offender>,
    /// Per-file rows. The dev dashboard draws ONE BUBBLE PER FILE from these,
    /// sizing by `loc`, colouring by `avg_complexity` and badging `dep_count`,
    /// so this section is what keeps the metrics visualisation alive when the
    /// per-framework metrics modules are retired (ADR-0002).
    pub(crate) file_metrics: Vec<FileMetrics>,
    /// Top 15 by complexity, for the "most complex functions" table.
    pub(crate) most_complex_functions: Vec<FunctionInfo>,
    /// file -> imported files, both sides relative paths inside the scan.
    pub(crate) dependency_graph: BTreeMap<String, Vec<String>>,
    /// Maximal duplicated AST shapes, biggest payoff first.
    pub(crate) duplication: Vec<CloneGroup>,
    /// Files excluded from every number above because too little of them
    /// parsed. Present even when empty so a consumer can rely on the key.
    pub(crate) unparsed: Vec<RefusedFile>,
    /// What changed since the previous recorded run of the SAME scan root.
    /// `null` on the first run (nothing to compare) or when `--no-history` is
    /// set. See the run-history section below.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delta: Option<RunDelta>,
}

pub(crate) struct Report {
    pub(crate) files: Vec<FileMetrics>,
    pub(crate) functions: Vec<FunctionInfo>,
    pub(crate) offenders: Vec<Offender>,
    pub(crate) scan_root: String,
    /// file -> the files it imports, both sides RELATIVE PATHS in the scanned
    /// set. Keys and values live in the same namespace on purpose: the previous
    /// implementation keyed this on module names while looking it up by path, so
    /// nothing ever matched and the dependency view drew no edges at all.
    pub(crate) dependency_graph: BTreeMap<String, Vec<String>>,
    pub(crate) clones: Vec<CloneGroup>,
    pub(crate) refused: Vec<RefusedFile>,
}

pub(crate) fn analyze_targets(files: &[PathBuf], scan_root: &str) -> Report {
    let test_index = build_test_index(scan_root);
    let mut file_metrics: Vec<FileMetrics> = Vec::new();
    let mut all_functions: Vec<FunctionInfo> = Vec::new();
    // Pass 1 cannot resolve imports: a specifier only becomes an edge once every
    // file in the scan is known. Park the raw specifiers with their language.
    let mut pending: Vec<(String, Lang, Vec<String>)> = Vec::new();
    // Duplication is the one metric that cannot be computed a file at a time, so
    // fragments accumulate across the whole scan and are grouped in pass 2.
    // `clone_paths` is the index space `CloneFragment.file` points into.
    let mut fragments: Vec<CloneFragment> = Vec::new();
    let mut clone_paths: Vec<String> = Vec::new();
    let mut refused: Vec<RefusedFile> = Vec::new();

    for path in files {
        let Some(lang) = Lang::from_path(path) else { continue };
        let Ok(source) = fs::read_to_string(path) else { continue };
        // A bundle that is not NAMED like one still must not skew the report.
        if looks_minified(&source) {
            continue;
        }
        let rel = rel_display(path, scan_root);
        let declared = declared_reference_names(&source, lang);
        let has_tests = (lang == Lang::Rust && rust_has_inline_tests(&source))
            || module_has_tests(path, &test_index, &declared);
        match analyze_source(lang, &source, &rel, has_tests) {
            // The engine could not read the file. It is kept OUT of every
            // aggregate - one file's rubble must not move another file's
            // numbers - and reported by name, because "could not read this" and
            // "this file is fine" must never look the same in the output.
            Some(FileAnalysis::Refused { reason, parse_health }) => {
                refused.push(RefusedFile {
                    path: rel,
                    parse_health,
                    lines: source.lines().count(),
                    reason,
                });
            }
            Some(FileAnalysis::Measured { metrics, functions, imports, fragments: frags }) => {
                // Fragments come back index-less; only this loop knows which
                // slot in `clone_paths` the file took.
                let idx = clone_paths.len() as u32;
                clone_paths.push(metrics.path.clone());
                fragments.extend(frags.into_iter().map(|f| CloneFragment { file: idx, ..f }));
                pending.push((metrics.path.clone(), lang, imports));
                file_metrics.push(metrics);
                all_functions.extend(functions);
            }
            // No grammar, or the parser returned nothing at all.
            None => {}
        }
    }

    // ── Pass 2: resolve specifiers to files, then derive the coupling triple ──
    let paths: HashSet<String> = file_metrics.iter().map(|f| f.path.clone()).collect();
    // The scan root's own basename is the implicit package prefix, so an absolute
    // intra-package import (`tina4_python.debug` while scanning tina4_python/)
    // still resolves.
    let root_pkg = Path::new(scan_root)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());

    let mut dependency_graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut afferent: HashMap<String, usize> = HashMap::new();

    for (from_rel, lang, specs) in &pending {
        let mut targets: Vec<String> = Vec::new();
        for spec in specs {
            if let Some(target) =
                resolve_import(spec, from_rel, *lang, &paths, root_pkg.as_deref())
            {
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
        for t in &targets {
            *afferent.entry(t.clone()).or_insert(0) += 1;
        }
        targets.sort();
        dependency_graph.insert(from_rel.clone(), targets);
    }

    for fm in file_metrics.iter_mut() {
        let ce = dependency_graph.get(&fm.path).map_or(0, |v| v.len());
        let ca = *afferent.get(&fm.path).unwrap_or(&0);
        fm.coupling_efferent = ce;
        fm.coupling_afferent = ca;
        fm.instability = if ca + ce > 0 {
            round_dp(ce as f64 / (ca + ce) as f64, 3)
        } else {
            // No internal edges either way: the file is isolated, not unstable.
            0.0
        };
    }

    let clones = group_clones(fragments, &clone_paths);
    let mut offenders = build_offenders(&file_metrics, &all_functions);
    offenders.extend(clone_offenders(&clones));
    offenders.extend(refusal_offenders(&refused));
    sort_offenders(&mut offenders);

    Report {
        files: file_metrics,
        functions: all_functions,
        offenders,
        scan_root: scan_root.to_string(),
        dependency_graph,
        clones,
        refused,
    }
}

pub(crate) fn build_summary(report: &Report, total_offenders: usize) -> Summary {
    let total_cc: u32 = report.functions.iter().map(|f| f.complexity).sum();
    let avg_complexity = if report.functions.is_empty() {
        0.0
    } else {
        round_dp(total_cc as f64 / report.functions.len() as f64, 2)
    };
    let total_mi: f64 = report.files.iter().map(|f| f.maintainability).sum();
    let avg_maintainability = if report.files.is_empty() {
        0.0
    } else {
        round_dp(total_mi / report.files.len() as f64, 1)
    };
    Summary {
        files_analyzed: report.files.len(),
        total_functions: report.functions.len(),
        avg_complexity,
        avg_maintainability,
        scan_mode: "project".to_string(),
        scan_root: report.scan_root.clone(),
        total_offenders,
        duplicate_blocks: report.clones.len(),
        duplicate_lines: report.clones.iter().map(|c| c.lines * (c.copies - 1)).sum(),
        files_refused: report.refused.len(),
    }
}
