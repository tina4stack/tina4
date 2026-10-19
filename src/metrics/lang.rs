// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Language detection, LOC (code-line) counting, and the rounding +
//! maintainability-index formula. Split out of the metrics engine (ADR-0002)
//! with no behaviour change; the formulas still mirror the retired metrics.py.

use std::path::Path;

// ── Languages ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Lang {
    Python,
    Php,
    Ruby,
    Ts, // TypeScript + TSX + JavaScript (parsed with the tsx grammar)
    Rust,
}

impl Lang {
    pub(crate) fn from_path(path: &Path) -> Option<Lang> {
        match path.extension().and_then(|e| e.to_str())?.to_ascii_lowercase().as_str() {
            "py" | "pyw" => Some(Lang::Python),
            "php" => Some(Lang::Php),
            "rb" => Some(Lang::Ruby),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Some(Lang::Ts),
            "rs" => Some(Lang::Rust),
            _ => None,
        }
    }

    pub(crate) fn tree_sitter_language(self) -> tree_sitter::Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Lang::Ts => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }
}

// ── Rounding + MI formula (mirror metrics.py exactly) ─────────────────────────

pub(crate) fn round_dp(value: f64, dp: i32) -> f64 {
    let m = 10f64.powi(dp);
    (value * m).round() / m
}

/// MI = max(0, min(100, (171 - 5.2*ln(V) - 0.23*CC - 16.2*ln(LOC)) * 100/171)).
/// `V` is Halstead volume, `avg_cc` the mean per-function complexity, `loc` the
/// code-line count. Identical to `_maintainability_index` in metrics.py.
pub(crate) fn maintainability_index(volume: f64, avg_cc: f64, loc: usize) -> f64 {
    if loc == 0 {
        return 100.0;
    }
    let v = volume.max(1.0);
    let mi = 171.0 - 5.2 * v.ln() - 0.23 * avg_cc - 16.2 * (loc as f64).ln();
    (mi * 100.0 / 171.0).clamp(0.0, 100.0)
}

// ── LOC (code lines) ──────────────────────────────────────────────────────────
// Python rule is byte-for-byte what metrics.py uses: a line counts unless it is
// blank or (trimmed) starts with `#`. The others use the analogous line-comment
// prefixes for their language.

pub(crate) fn is_code_line(line: &str, lang: Lang) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    match lang {
        Lang::Python | Lang::Ruby => !t.starts_with('#'),
        Lang::Php => !(t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*')),
        // Rust shares TypeScript's comment shapes exactly: `//` also covers the
        // doc forms `///` and `//!`, `/*` opens a block (which nests in Rust, but
        // that only matters to the parser, not to a line-prefix count), and a
        // leading `*` is the block-continuation convention.
        //
        // Known, deliberate limitation, identical to the one PHP and TS already
        // carry: a line that STARTS with a dereference (`*counter = 0;`) is read
        // as a comment continuation. Rust hits this more often than TS does, but
        // the point of ADR-0002 is one definition of LOC for every language, so
        // Rust does not get a private rule. It undercounts by a handful of lines
        // in the worst file, never enough to move an MI band.
        Lang::Ts | Lang::Rust => !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')),
    }
}

pub(crate) fn count_loc(source: &str, lang: Lang) -> usize {
    source.lines().filter(|l| is_code_line(l, lang)).count()
}
