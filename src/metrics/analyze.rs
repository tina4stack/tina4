// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-file analyze_source orchestration. Split out of the metrics engine (ADR-0002); behaviour
//! unchanged. `use super::*` shares Lang and the tree-sitter types.

use super::*;

// ── Analyze one source string ─────────────────────────────────────────────────

/// What one file contributed to the report.
///
/// A single verdict, decided in ONE place, is the whole point. The engine used
/// to answer "did this file parse?" with `Option` (grammar missing) and then
/// silently report numbers for anything that came back `Some` - including a file
/// tree-sitter had wrapped almost entirely in ERROR nodes. Refusal has to be a
/// value the caller must handle, not an absence it can overlook.
pub(crate) enum FileAnalysis {
    Measured {
        metrics: FileMetrics,
        functions: Vec<FunctionInfo>,
        imports: Vec<String>,
        /// Clone fragments with `file` left at 0; only the caller knows the
        /// index. Computed here so the tree is parsed ONCE per file instead of
        /// once for metrics and again for duplication.
        fragments: Vec<CloneFragment>,
    },
    /// Read, and deliberately NOT reported on.
    Refused { reason: String, parse_health: f64 },
}

pub(crate) fn analyze_source(
    lang: Lang,
    source: &str,
    rel_path: &str,
    has_tests: bool,
) -> Option<FileAnalysis> {
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language()).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let src = source.as_bytes();

    // ── Refusal gate 1: nesting the recursive walks below cannot survive. ──
    // Checked BEFORE any of them run, because the failure mode is aborting the
    // process, not returning a wrong number.
    if depth_exceeds(root, MAX_AST_DEPTH) {
        return Some(FileAnalysis::Refused {
            reason: format!(
                "AST nests deeper than {MAX_AST_DEPTH} levels - measuring it would overflow the stack"
            ),
            // Nothing is known about its parse health; it was never walked.
            parse_health: 0.0,
        });
    }

    // ── Refusal gate 2: too much of the file sits inside parse-error regions. ──
    // Also before the walks, so a file the engine cannot read costs it nothing.
    let health = round_dp(parse_health(root, source.lines().count()), 3);
    if health < MIN_PARSE_HEALTH {
        return Some(FileAnalysis::Refused {
            reason: format!(
                "only {:.0}% of lines parsed - metrics would be wrong, not just imprecise",
                health * 100.0
            ),
            parse_health: health,
        });
    }

    let loc = count_loc(source, lang);

    let mut fn_nodes: Vec<(Node, u32)> = Vec::new();
    collect_functions(root, lang, src, &mut fn_nodes);

    let mut functions: Vec<FunctionInfo> = Vec::with_capacity(fn_nodes.len());
    let mut file_complexity: u32 = 0;
    for (node, cc) in &fn_nodes {
        file_complexity += *cc;
        let start = node.start_position().row;
        let end = node.end_position().row;
        // Count CODE lines in the function's span, using the same is_code_line
        // rule as the file-level LOC so the two numbers are comparable.
        let fn_loc = source
            .lines()
            .skip(start)
            .take(end.saturating_sub(start) + 1)
            .filter(|l| is_code_line(l, lang))
            .count();
        functions.push(FunctionInfo {
            name: function_display_name(*node, lang, src),
            file: rel_path.to_string(),
            line: start + 1,
            complexity: *cc,
            loc: fn_loc,
        });
    }

    let num_functions = fn_nodes.len();
    let avg_cc = if num_functions > 0 {
        file_complexity as f64 / num_functions as f64
    } else {
        0.0
    };

    let vol = file_volume(root, src, lang);
    let mi = round_dp(maintainability_index(vol, avg_cc, loc), 1);
    let specs = extract_import_specs(root, lang, src);

    let fm = FileMetrics {
        path: rel_path.to_string(),
        loc,
        complexity: file_complexity,
        avg_complexity: round_dp(avg_cc, 2),
        functions: num_functions,
        maintainability: mi,
        halstead_volume: round_dp(vol, 2),
        has_referencing_test: has_tests,
        // dep_count is every import as written; the coupling triple is filled in
        // by the second pass in analyze_targets, which alone knows every file.
        dep_count: specs.len(),
        coupling_efferent: 0,
        coupling_afferent: 0,
        instability: 0.0,
        parse_health: health,
    };

    let mut fragments: Vec<CloneFragment> = Vec::new();
    collect_fragments(root, 0, src, &mut fragments);

    Some(FileAnalysis::Measured { metrics: fm, functions, imports: specs, fragments })
}
