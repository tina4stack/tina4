// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cross-file duplicate detection via AST-shape hashing. Split out of the metrics engine (ADR-0002); behaviour
//! unchanged. `use super::*` shares Lang and the tree-sitter types.

use super::*;

// ── Duplication (DRY) ─────────────────────────────────────────────────────────
//
// Cross-file, AST-shape based, and therefore language-agnostic by construction:
// it works for all five languages through the same code path, and a sixth would
// need no work here. This is the Baxter et al. approach - hash sub-trees, group
// by hash - rather than the token-window scan PMD's CPD uses, because the engine
// already has a real parse and a shape hash is immune to reformatting.
//
// WHAT IT DETECTS, measured against tree-sitter 0.26.11 rather than asserted.
// Hashing the sequence of node KINDS with all identifier and literal TEXT
// excluded gives TYPE-1 PLUS CONSISTENT RENAMING: two blocks that differ only in
// their variable names, in same-kind literal values, or in whitespace and
// indentation collide, which is the point.
//
// It implements Type-2 comment tolerance by removing comment/extra nodes from
// both the hash and node count. Python docstrings remain significant because
// they are executable string expressions, not parser comments.
//
// Type-3 (gapped) and Type-4 (semantic) clones are NOT detected at all; that
// needs sub-tree differencing, and a report that silently claimed to cover them
// would be worse than one with a stated ceiling.
//
// Anonymous token kinds ARE hashed, so `a + b` and `x - y` do not collide even
// though both are a binary expression over two identifiers. Excluding operators
// was the obvious simplification and it is exactly what makes a shape hash
// notorious for false positives.
//
// tina4: thresholds below are the one genuinely tunable decision here. They are
// deliberately expressed as two INDEPENDENT gates because either alone is known
// to misbehave: a node-count gate alone fires on a long flat list of struct
// fields, and a line gate alone fires on any six sparsely-formatted lines.

/// Minimum AST nodes in a sub-tree before it can be reported as duplicated.
///
/// Sits between jscpd's 50-token default and PMD CPD's 100-token Java default.
/// AST nodes are finer-grained than source tokens (an `if` statement is several
/// nodes), so a node count maps to roughly half as many tokens: 60 nodes is on
/// the order of 30-40 tokens of real code.
pub(crate) const MIN_CLONE_NODES: u32 = 60;

/// Minimum SOURCE LINES a duplicate must span. Matches SonarQube's 10-line
/// default intent while staying tighter, and it is what stops a one-line
/// accessor or a short guard clause from ever being reported however many times
/// it appears.
pub(crate) const MIN_CLONE_LINES: usize = 6;

/// One duplicated region: where it is and how big.
///
/// `file` is an INDEX into the scanned file list rather than a String. With one
/// fragment per qualifying sub-tree, a large project produces hundreds of
/// thousands of these, and a cloned path on each is the difference between a
/// few MB and a few hundred. The hash is a u64 for the same reason - the
/// normalised shape string is never retained, only folded in.
#[derive(Clone, Debug)]
pub(crate) struct CloneFragment {
    pub(crate) hash: u64,
    pub(crate) file: u32,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) nodes: u32,
}

impl CloneFragment {
    fn lines(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }
    /// Does this fragment sit inside `other`, INCLUDING covering the exact same
    /// span?
    ///
    /// The equal-span case is the one that matters and it is easy to get wrong.
    /// A single duplicated block produces a fragment at every wrapper level that
    /// shares its line range - `expression_statement` around `call_expression`
    /// around the arguments - each with a DIFFERENT shape hash, so they form
    /// separate groups that all report the same lines. Excluding equal spans
    /// (the obvious reading of "strictly inside") let all of them through and the
    /// same clone was listed three or four times.
    ///
    /// Self-suppression is not a risk: a group is only ever tested against
    /// groups already KEPT, never against itself.
    fn inside(&self, other: &CloneFragment) -> bool {
        self.file == other.file
            && self.start_line >= other.start_line
            && self.end_line <= other.end_line
    }
}

/// Fold a node's kind and its children's hashes into one structural hash.
///
/// Bottom-up and single-pass: every node's hash is built from the hashes its
/// children already produced, so the whole file costs O(nodes). Serialising each
/// candidate sub-tree to a string and hashing that would be O(nodes * depth) and
/// would allocate a string per candidate.
pub(crate) fn shape_hash_of(kind: &str, child_hashes: &[u64]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    kind.hash(&mut h);
    // Length is folded in so a node with two children cannot collide with a
    // differently-shaped node whose child hashes happen to concatenate the same.
    child_hashes.len().hash(&mut h);
    for c in child_hashes {
        c.hash(&mut h);
    }
    h.finish()
}

/// Leaf kinds that carry opaque string BODY text across the five grammars. Their
/// content is folded into the clone hash (below) so two blocks whose only shared
/// structure is a string/template literal do not collide. Identifiers are
/// deliberately absent: renaming them must still read as the same shape, which is
/// what makes this a Type-2 (renamed-clone) detector rather than an exact-match one.
pub(crate) fn is_string_leaf(kind: &str) -> bool {
    matches!(
        kind,
        "string_fragment"        // ts/js template and string bodies
            | "string_content"   // python, ruby, rust, php
            | "string_value"     // php alternate
            | "heredoc_body"     // php / ruby heredocs
            | "raw_string_literal" // rust r"..."
    )
}

/// Fold a string leaf's text into its structural hash. A string BODY is data, not
/// renameable code, so two blocks that differ only in their string contents are
/// not "one block to unify". Without this, two unrelated template literals that
/// share the same `${...}` interpolation skeleton hash identically and get
/// reported as a phantom clone (the embedded-worker-source false positive that
/// `mongoClient.ts` and `syncBridge.ts` tripped). Identical strings still fold to
/// the identical hash, so real clones are detected exactly as before.
pub(crate) fn mix_leaf_text(seed: u64, text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    seed.hash(&mut h);
    text.hash(&mut h);
    h.finish()
}

/// Walk one file's tree, emitting a fragment for every sub-tree big enough to be
/// worth reporting. Returns this node's (hash, node_count, parsed_cleanly).
///
/// `parsed_cleanly` is false for any sub-tree containing an ERROR or MISSING
/// node, and such a sub-tree NEVER becomes a fragment. A shape hashed out of a
/// misparse describes the parser's recovery, not the author's code, so two files
/// whose only similarity is being broken would otherwise be reported as
/// duplicates of each other. Measured on tree-sitter 0.26.11: seven lines of
/// pure Python garbage fold into a 69-node ERROR node spanning 7 lines, clearing
/// both gates below on its own.
///
/// This is belt AND braces with the parse-health refusal in `analyze_source`,
/// deliberately. The refusal drops whole files below a THRESHOLD; this makes the
/// guarantee unconditional, so it still holds for the error regions inside a
/// file healthy enough to be reported, and it does not quietly weaken if that
/// threshold is ever tuned.
pub(crate) fn collect_fragments(
    node: Node,
    file: u32,
    src: &[u8],
    out: &mut Vec<CloneFragment>,
) -> (u64, u32, bool) {
    if node.is_error() || node.is_missing() {
        return (shape_hash_of(node.kind(), &[]), 1, false);
    }
    if node.is_extra() || node.kind().contains("comment") {
        return (0, 0, true);
    }
    let mut child_hashes: Vec<u64> = Vec::new();
    let mut count: u32 = 1;
    let mut parsed_cleanly = !node.is_error() && !node.is_missing();
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            let (h, n, clean) = collect_fragments(c.node(), file, src, out);
            if n > 0 {
                child_hashes.push(h);
                count += n;
            }
            parsed_cleanly &= clean;
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
    let mut hash = shape_hash_of(node.kind(), &child_hashes);
    // A string leaf's bytes distinguish it: two different bodies must not collide
    // on a shared AST shape (the template-literal false positive). Identical text
    // folds to the identical hash, so genuine clones are untouched.
    if is_string_leaf(node.kind()) {
        if let Ok(text) = node.utf8_text(src) {
            hash = mix_leaf_text(hash, text);
        }
    }
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    // Only NAMED nodes are candidates: an anonymous token is punctuation and can
    // never be a meaningful duplicate on its own, though its kind still shapes
    // the hash of the parent that contains it.
    if parsed_cleanly
        && node.is_named()
        && count >= MIN_CLONE_NODES
        && end_line.saturating_sub(start_line) + 1 >= MIN_CLONE_LINES
    {
        out.push(CloneFragment { hash, file, start_line, end_line, nodes: count });
    }
    (hash, count, parsed_cleanly)
}

/// A set of fragments that share a shape.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct CloneOccurrence {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CloneGroup {
    pub files: Vec<String>,
    pub first_file: String,
    pub first_line: usize,
    pub copies: usize,
    pub lines: usize,
    pub cross_file: bool,
    /// Every source occurrence in this clone group. The legacy summary fields
    /// remain unchanged; this makes the JSON actionable instead of naming only
    /// the first occurrence and the participating files.
    pub occurrences: Vec<CloneOccurrence>,
}

/// Group fragments by shape, then keep only the MAXIMAL ones.
///
/// The suppression pass is not an optimisation, it is what makes the output
/// readable. If a 200-node block is duplicated then so is every one of its
/// sub-blocks, so a raw grouping reports the same clone at a dozen nesting
/// depths and buries the finding. A group is dropped when every one of its
/// fragments sits inside a fragment of some larger surviving group.
pub(crate) fn group_clones(mut frags: Vec<CloneFragment>, paths: &[String]) -> Vec<CloneGroup> {
    // Largest first, so a parent is always considered before its children.
    frags.sort_by(|a, b| b.nodes.cmp(&a.nodes).then(a.file.cmp(&b.file)).then(a.start_line.cmp(&b.start_line)));

    let mut by_hash: BTreeMap<u64, Vec<CloneFragment>> = BTreeMap::new();
    for f in frags {
        by_hash.entry(f.hash).or_default().push(f);
    }

    // Only shapes that actually repeat.
    let mut candidates: Vec<Vec<CloneFragment>> =
        by_hash.into_values().filter(|v| v.len() >= 2).collect();
    candidates.sort_by(|a, b| b[0].nodes.cmp(&a[0].nodes));

    let mut kept: Vec<Vec<CloneFragment>> = Vec::new();
    for group in candidates {
        let covered = group.iter().all(|f| {
            kept.iter().any(|k| k.iter().any(|big| f.inside(big)))
        });
        if !covered {
            kept.push(group);
        }
    }

    let mut out: Vec<CloneGroup> = kept
        .into_iter()
        .map(|g| {
            let mut files: Vec<String> = g
                .iter()
                .map(|f| paths.get(f.file as usize).cloned().unwrap_or_default())
                .collect();
            files.sort();
            files.dedup();
            let first = &g[0];
            let mut occurrences: Vec<CloneOccurrence> = g
                .iter()
                .map(|f| CloneOccurrence {
                    file: paths.get(f.file as usize).cloned().unwrap_or_default(),
                    start_line: f.start_line,
                    end_line: f.end_line,
                })
                .collect();
            occurrences.sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
            CloneGroup {
                cross_file: files.len() > 1,
                first_file: paths.get(first.file as usize).cloned().unwrap_or_default(),
                first_line: first.start_line,
                copies: g.len(),
                lines: first.lines(),
                files,
                occurrences,
            }
        })
        .collect();
    // Biggest, most-repeated first - that is the order someone fixing them wants.
    out.sort_by(|a, b| {
        (b.lines * b.copies)
            .cmp(&(a.lines * a.copies))
            .then(b.copies.cmp(&a.copies))
            .then(a.first_file.cmp(&b.first_file))
    });
    out
}
