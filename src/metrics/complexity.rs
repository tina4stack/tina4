// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parse health, McCabe cyclomatic complexity, and the function/class node
//! detection + display-name walk. Split out of the metrics engine (ADR-0002);
//! behaviour unchanged. `use super::*` shares Lang and the tree-sitter types.

use super::*;

// ── Parse health ──────────────────────────────────────────────────────────────
//
// tree-sitter ALWAYS returns a tree. When it cannot parse something it wraps the
// region in an ERROR node and carries on, which is the right behaviour for an
// editor and a trap for a metrics engine: every number downstream is still
// computed, still plausible, and quietly wrong, because the decision points and
// operators inside an ERROR region are invisible to the walks that count them.
//
// A file the engine cannot read must therefore be REFUSED and said so, never
// silently reported and never silently skipped. This was found via Delphi (a
// grammar gap put 51.5% of a real corpus inside ERROR regions) but it is not a
// Delphi problem - it applies to all five shipping languages equally, and a
// malformed .py or a .ts using syntax newer than the vendored grammar hits it
// the same way.

/// Fraction of source lines that parsed cleanly: 1.0 is a perfect parse, 0.0 is
/// a file entirely inside error regions.
///
/// Measured by LINE COVERAGE rather than by counting ERROR nodes, because the
/// two diverge badly. One ERROR node can swallow a thousand lines, and a
/// thousand tiny ones can sit on a single line; only the span says how much of
/// the file the engine actually understood.
///
/// Children of an ERROR node are not descended into - they are junk by
/// definition, and the parent's span already covers them.
pub(crate) fn parse_health(root: Node, total_lines: usize) -> f64 {
    if total_lines == 0 {
        return 1.0;
    }
    let mut bad: HashSet<usize> = HashSet::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        // MISSING nodes are zero-width insertions the parser invented to
        // recover. They mark a real defect but cover no source, so they are
        // counted at their own line rather than over a span.
        if n.is_error() || n.is_missing() {
            for row in n.start_position().row..=n.end_position().row {
                bad.insert(row);
            }
            if n.is_error() {
                continue;
            }
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    let clean = total_lines.saturating_sub(bad.len());
    (clean as f64 / total_lines as f64).clamp(0.0, 1.0)
}

/// Below this fraction of cleanly-parsed lines a file is REFUSED rather than
/// reported.
///
/// Calibrated against every corpus to hand rather than guessed. Measured over
/// 1,875 files in tina4-python, tina4-php, tina4-ruby, tina4-nodejs, tina4-js
/// and this CLI's own src:
///
/// * 1,873 parse at health EXACTLY 1.000.
/// * 2 do not, and both sit far above the floor: 0.993
///   (`tina4-php/tests/OrmQueryBuilderBugsPostgresTest.php`) and 0.994
///   (`tina4-nodejs/types/core/src/devAdmin.d.ts`, a `.d.ts` read with the TSX
///   grammar). Real grammar gaps, and small ones.
/// * NONE is below 0.95.
///
/// So the gap between healthy and broken is not a gradient to tune along, it is
/// a cliff, and 0.95 sits in the empty space under it with the nearest real file
/// four points clear. It tolerates a stray region from one unsupported construct
/// - where the surrounding metrics are still broadly meaningful - and refuses
/// anything structurally misread. It is deliberately NOT 1.0: that would refuse
/// those two real files over a single unrecognised construct, and make the
/// engine useless on any codebase slightly ahead of its vendored grammars.
pub(crate) const MIN_PARSE_HEALTH: f64 = 0.95;

/// Deepest AST nesting the engine's RECURSIVE walks will attempt.
///
/// Five walks recurse once per tree level - `collect_functions`,
/// `count_own_decisions`, `py_halstead`, `generic_halstead` and
/// `collect_fragments` - and none of them had a bound. Measured on macOS 26.5.2
/// arm64 against the 58f7f73 release binary: a 60,000-term left-associative
/// Python expression written one term per line parses fine (tree depth 60,003),
/// then aborts the whole process with `fatal runtime error: stack overflow`,
/// exit 134. Nothing upstream catches it - the file is 10 bytes per line, so
/// `looks_minified` (threshold 200) does not fire.
///
/// This is a PRE-EXISTING defect, not something duplication introduced: the
/// three walks that feed LOC / CC / MI have recursed unguarded since the engine
/// was written, so the crash predates both `cd4dae8` and `58f7f73`.
///
/// 800 is bracketed by two measurements rather than picked:
///
/// * DEEPEST REAL FILE = 79, `src/agent.rs`, over 1,875 files in tina4-python,
///   tina4-php, tina4-ruby, tina4-nodejs, tina4-js and this CLI's own src.
///   Per-repo maxima are 50 / 50 / 35 / 56 / 31 / 79. Ten times the deepest
///   thing anyone has actually written is not a limit real code can hit.
/// * FIRST ABORT = depth 1800, measured in the harshest environment the walks
///   run in - a DEBUG build on a 2 MiB cargo-test worker thread (depth 1700
///   still completes). The release binary on the 8 MiB main thread survives far
///   more. Half the worst-case ceiling leaves the guard itself with margin.
///
/// A file past it is REFUSED like any other the engine cannot read. Refusal
/// names the file; a crash names nothing and loses the entire scan with it.
pub(crate) const MAX_AST_DEPTH: usize = 800;

/// Does any node nest deeper than `limit`?
///
/// Deliberately ITERATIVE. A recursive depth check would overflow the very stack
/// it exists to protect, on exactly the input it exists to catch.
pub(crate) fn depth_exceeds(root: Node, limit: usize) -> bool {
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        if depth > limit {
            return true;
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            stack.push((child, depth + 1));
        }
    }
    false
}

// ── Cyclomatic complexity ─────────────────────────────────────────────────────
// CC = 1 + decision points, counted over the function's ENTIRE subtree (nested
// functions included — matching ast.walk in metrics.py). Python's decision set is
// replicated exactly; the other languages use the same McCabe definition applied
// through their grammar (if/loop/case/catch/ternary/&&/||).

pub(crate) fn is_boolean_binary(node: Node, src: &[u8]) -> bool {
    matches!(operator_token(node, src), Some("&&") | Some("||") | Some("and") | Some("or"))
}

pub(crate) fn is_decision(node: Node, lang: Lang, src: &[u8]) -> u32 {
    // Anonymous nodes are the literal keyword tokens. In tree-sitter's Ruby
    // grammar "if" / "unless" / "while" / "when" / "rescue" name BOTH a construct
    // and its keyword, so matching on kind alone counted every Ruby decision
    // twice - `return 1 if y` scored 3 instead of 2, and every Ruby complexity
    // number came out roughly double.
    if !node.is_named() {
        return 0;
    }
    let k = node.kind();
    match lang {
        Lang::Python => matches!(
            k,
            "if_statement"
                | "elif_clause"
                | "for_statement"
                | "while_statement"
                | "except_clause"
                | "assert_statement"
                | "conditional_expression"
                | "boolean_operator"
                | "for_in_clause"
                | "if_clause"
        ) as u32,
        Lang::Php => {
            if matches!(
                k,
                "if_statement"
                    | "else_if_clause"
                    | "for_statement"
                    | "foreach_statement"
                    | "while_statement"
                    | "do_statement"
                    | "case_statement"
                    | "catch_clause"
                    | "conditional_expression"
            ) {
                1
            } else {
                (k == "binary_expression" && is_boolean_binary(node, src)) as u32
            }
        }
        Lang::Ruby => {
            if matches!(
                k,
                "if" | "elsif" | "unless" | "while" | "until" | "for" | "when" | "rescue"
                    | "conditional" | "if_modifier" | "unless_modifier" | "while_modifier"
                    | "until_modifier"
            ) {
                1
            } else {
                (k == "binary" && is_boolean_binary(node, src)) as u32
            }
        }
        Lang::Ts => {
            if matches!(
                k,
                "if_statement"
                    | "for_statement"
                    | "for_in_statement"
                    | "while_statement"
                    | "do_statement"
                    | "switch_case"
                    | "catch_clause"
                    | "ternary_expression"
            ) {
                1
            } else {
                (k == "binary_expression" && is_boolean_binary(node, src)) as u32
            }
        }
        // Node kinds verified against tree-sitter-rust 0.24.2 by dumping a real
        // parse, not from memory. Three of them are easy to get wrong:
        //
        //  * `if let` / `while let` do NOT have their own node kinds. The grammar
        //    reuses `if_expression` / `while_expression` with a `let_condition`
        //    child, so matching the plain kinds already covers both. Adding an
        //    `if_let_expression` arm would have matched nothing and looked fine.
        //  * `else_clause` is NOT counted. `else` is the fall-through edge, not a
        //    decision; `else if` nests a second `if_expression` inside the clause
        //    and is counted there, so `if/else if/else` scores 2, which is right.
        //  * `try_expression` is the `?` operator. It is a real early-return
        //    branch and is invisible if you only look for keywords - this is the
        //    one the brief flagged and it is worth 1 per `?`.
        //
        // `loop_expression` has no condition but still closes a cycle in the
        // control-flow graph, so it earns its point like any other loop.
        Lang::Rust => {
            if matches!(
                k,
                "if_expression"
                    | "while_expression"
                    | "loop_expression"
                    | "for_expression"
                    | "try_expression"
            ) {
                1
            } else if k == "match_arm" {
                // Every arm is a branch EXCEPT the wildcard `_`, which is the
                // fall-through. This follows the precedent already set for
                // TypeScript, where tree-sitter names `default:` `switch_default`
                // (not `switch_case`) and the engine therefore never counts it.
                // Verified: `match { 1 => .., 2 | 3 => .., _ => .. }` scores 2.
                (!is_rust_wildcard_arm(node, src)) as u32
            } else {
                (k == "binary_expression" && is_boolean_binary(node, src)) as u32
            }
        }
    }
}

/// Is this `match_arm` the catch-all `_ => ...`?
///
/// Compared on the pattern's TEXT rather than its node kind: the kind name for a
/// wildcard has moved between grammar versions, and `_` is unambiguous.
pub(crate) fn is_rust_wildcard_arm(node: Node, src: &[u8]) -> bool {
    node.child_by_field_name("pattern")
        .and_then(|p| p.utf8_text(src).ok())
        .map(|t| t.trim() == "_")
        .unwrap_or(false)
}

/// True for a node that opens a new measurement scope: a nested function (it is
/// reported as a function in its own right) or a class body (its methods are).
/// Mirrors metrics.py, which skips FunctionDef / AsyncFunctionDef / ClassDef.
pub(crate) fn is_scope_boundary(node: Node, lang: Lang) -> bool {
    let kind = node.kind();
    if node.is_named() && is_function_node(kind, lang) {
        return true;
    }
    match lang {
        Lang::Python => kind == "class_definition",
        Lang::Php => matches!(kind, "class_declaration" | "interface_declaration"
            | "trait_declaration" | "enum_declaration"),
        Lang::Ruby => matches!(kind, "class" | "module"),
        Lang::Ts => matches!(kind, "class_declaration" | "class"),
        Lang::Rust => is_class_node(kind, Lang::Rust),
    }
}

/// Decision points in this function's OWN body.
///
/// Descent stops at a nested function or class, because those are measured
/// separately. Counting them here as well charged a single branch to two
/// different functions, and the over-count compounded with nesting depth: an
/// IIFE wrapper or a registrar defining twenty inner handlers absorbed the whole
/// file's complexity and topped the offenders list, hiding the real hot spots.
///
/// Every callable is a scope boundary. A branch belongs to the callable that
/// executes it, regardless of which supported language spells that callable.
pub(crate) fn count_own_decisions(node: Node, lang: Lang, src: &[u8]) -> u32 {
    let mut total = is_decision(node, lang, src);
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            let child = c.node();
            if !is_scope_boundary(child, lang) {
                total += count_own_decisions(child, lang, src);
            }
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
    total
}

// ── Functions ─────────────────────────────────────────────────────────────────

pub(crate) fn is_function_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => matches!(kind, "function_definition" | "lambda"),
        Lang::Php => matches!(
            kind,
            "function_definition" | "method_declaration" | "anonymous_function" | "arrow_function"
        ),
        // Ruby's lambda node wraps a block/do_block node. Counting that body is
        // enough and avoids reporting the same lambda twice. Ordinary iterator
        // blocks use the same nodes and are callables too.
        Lang::Ruby => matches!(kind, "method" | "singleton_method" | "block" | "do_block"),
        Lang::Ts => matches!(
            kind,
            "function_declaration"
                | "generator_function_declaration"
                | "method_definition"
                | "function_expression"
                | "arrow_function"
        ),
        // `function_signature_item` (a bodyless trait method) IS counted, on the
        // precedent PHP already sets: tree-sitter-php reports an interface's
        // bodyless `public function sig();` as a `method_declaration`, and the
        // engine has always counted it.
        Lang::Rust => matches!(kind, "function_item" | "function_signature_item" | "closure_expression"),
    }
}

pub(crate) fn is_class_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => kind == "class_definition",
        Lang::Php => matches!(kind, "class_declaration" | "trait_declaration" | "interface_declaration"),
        Lang::Ruby => matches!(kind, "class" | "module"),
        Lang::Ts => matches!(kind, "class_declaration" | "class"),
        // `impl_item` is what gives a Rust method its qualifier: `impl Point`
        // and `impl Draw for Point` both carry the implementing type in a field
        // named `type` (NOT `name` - see `node_name`), so `fn draw` inside either
        // is reported as `Point.draw`.
        Lang::Rust => matches!(
            kind,
            "impl_item" | "trait_item" | "struct_item" | "enum_item" | "union_item"
        ),
    }
}

/// Does this node DECLARE a named type a test could reference by name?
///
/// Wider than `is_class_node` on purpose, and used only by test detection.
/// `is_class_node` also drives function naming (`Calculator.add`), so widening it
/// would start naming interface method signatures after their interface.
///
/// TypeScript needed this: an interface-only module has no class at all, so a
/// test that references it by name was reported UNTESTED and raised a false
/// offender. PHP already counted `interface_declaration` here.
pub(crate) fn is_type_decl_node(kind: &str, lang: Lang) -> bool {
    if is_class_node(kind, lang) && !(lang == Lang::Ruby && kind == "module") {
        return true;
    }
    match lang {
        Lang::Ts => matches!(kind, "interface_declaration" | "type_alias_declaration" | "enum_declaration"),
        // A Rust module's public surface is its types, and a test names them
        // directly (`metrics::FileMetrics`). `type_item` covers `type Alias = ..`.
        // Without this every .rs file raised a false "untested" offender, exactly
        // as an interface-only TypeScript module used to.
        Lang::Rust => kind == "type_item",
        _ => false,
    }
}

/// The declared name of a type or function node.
///
/// Falls back to the `type` field because Rust's `impl_item` has no `name`: both
/// `impl Point` and `impl Draw for Point` store the implementing type under
/// `type` (and the trait, when present, under `trait`). Without the fallback
/// every method in the CLI would have been reported bare as `run` or `new`
/// instead of `Report.run`. No other grammar in the set puts a `type` field on a
/// node this is called with, so the fallback cannot affect them.
pub(crate) fn node_name(node: Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
}

/// Outermost enclosing class name (matches metrics.py `_get_parent_class`, which
/// returns the BFS-first — i.e. outermost — containing class).
pub(crate) fn outer_class_name(node: Node, lang: Lang, src: &[u8]) -> Option<String> {
    let mut found: Option<String> = None;
    let mut cur = node.parent();
    while let Some(p) = cur {
        if is_class_node(p.kind(), lang) {
            if let Some(n) = node_name(p, src) {
                found = Some(n);
            }
        }
        cur = p.parent();
    }
    found
}

pub(crate) fn function_display_name(node: Node, lang: Lang, src: &[u8]) -> String {
    let base = node_name(node, src).unwrap_or_else(|| {
        // Arrow / anonymous: derive from `const name = () => …` or `name: () => …`.
        if let Some(parent) = node.parent() {
            if matches!(parent.kind(), "variable_declarator" | "pair" | "assignment_expression") {
                if let Some(n) = parent
                    .child_by_field_name("name")
                    .or_else(|| parent.child_by_field_name("key"))
                    .or_else(|| parent.child_by_field_name("left"))
                {
                    if let Ok(t) = n.utf8_text(src) {
                        return t.to_string();
                    }
                }
            }
        }
        "(anonymous)".to_string()
    });
    match outer_class_name(node, lang, src) {
        Some(cls) => format!("{cls}.{base}"),
        None => base,
    }
}

pub(crate) fn collect_functions<'a>(node: Node<'a>, lang: Lang, src: &[u8], out: &mut Vec<(Node<'a>, u32)>) {
    if node.is_named() && is_function_node(node.kind(), lang) {
        out.push((node, count_own_decisions(node, lang, src) + 1));
    }
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            collect_functions(c.node(), lang, src, out);
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
}
