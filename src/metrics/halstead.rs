// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Halstead volume + the per-language operator/operand token walk that feeds the
//! maintainability index. Split out of the metrics engine (ADR-0002); behaviour
//! unchanged. `use super::*` pulls in Lang and the tree-sitter types it shares
//! with the rest of the engine.

use super::*;

// ── Halstead volume ───────────────────────────────────────────────────────────

#[derive(Default)]
pub(crate) struct Halstead {
    total: usize,
    unique: HashSet<String>,
}
impl Halstead {
    fn add(&mut self, key: impl Into<String>) {
        self.total += 1;
        self.unique.insert(key.into());
    }
}

pub(crate) fn operator_token<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if !child.is_named() {
            return child.utf8_text(src).ok();
        }
    }
    None
}

pub(crate) fn volume(n1: usize, n2: usize, big_n1: usize, big_n2: usize) -> f64 {
    let vocabulary = n1 + n2;
    let length = big_n1 + big_n2;
    if vocabulary > 0 {
        (length as f64) * (vocabulary as f64).log2()
    } else {
        0.0
    }
}

// ---- Python-precise Halstead (mirrors ast.Name / ast.Constant + operator set) --

pub(crate) fn py_binop_name(tok: &str) -> &'static str {
    match tok {
        "+" => "Add", "-" => "Sub", "*" => "Mult", "/" => "Div", "%" => "Mod",
        "**" => "Pow", "//" => "FloorDiv", "<<" => "LShift", ">>" => "RShift",
        "&" => "BitAnd", "|" => "BitOr", "^" => "BitXor", "@" => "MatMult",
        _ => "BinOp",
    }
}

pub(crate) fn py_unary_name(tok: &str) -> &'static str {
    match tok {
        "-" => "USub", "+" => "UAdd", "~" => "Invert", _ => "UnaryOp",
    }
}

pub(crate) fn py_aug_name(tok: &str) -> &'static str {
    match tok {
        "+=" => "AugAdd", "-=" => "AugSub", "*=" => "AugMult", "/=" => "AugDiv",
        "%=" => "AugMod", "**=" => "AugPow", "//=" => "AugFloorDiv",
        "<<=" => "AugLShift", ">>=" => "AugRShift", "&=" => "AugBitAnd",
        "|=" => "AugBitOr", "^=" => "AugBitXor", "@=" => "AugMatMult",
        _ => "AugAssign",
    }
}

/// Anonymous operator tokens of a `comparison_operator`, folded so that
/// `is not` / `not in` are one operator each — matching ast's `node.ops`.
pub(crate) fn py_compare_ops(node: Node, src: &[u8]) -> Vec<&'static str> {
    let mut toks: Vec<String> = Vec::new();
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if !child.is_named() {
            toks.push(child.utf8_text(src).unwrap_or("").to_string());
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        match t {
            "is not" => out.push("IsNot"),
            "not in" => out.push("NotIn"),
            "is" => {
                if toks.get(i + 1).map(String::as_str) == Some("not") {
                    out.push("IsNot");
                    i += 1;
                } else {
                    out.push("Is");
                }
            }
            "not" => {
                if toks.get(i + 1).map(String::as_str) == Some("in") {
                    out.push("NotIn");
                    i += 1;
                } else {
                    out.push("Not");
                }
            }
            "<" => out.push("Lt"),
            "<=" => out.push("LtE"),
            ">" => out.push("Gt"),
            ">=" => out.push("GtE"),
            "==" => out.push("Eq"),
            "!=" | "<>" => out.push("NotEq"),
            "in" => out.push("In"),
            _ => {}
        }
        i += 1;
    }
    out
}

/// Does this `identifier` correspond to an `ast.Name` (a real operand)?  Excludes
/// declaration names, attribute tails, keyword-arg names, params, imports and
/// global/nonlocal — exactly the identifiers ast does NOT surface as `ast.Name`.
pub(crate) fn py_identifier_is_operand(parent_kind: Option<&str>, field: Option<&str>) -> bool {
    match parent_kind {
        Some("function_definition") | Some("class_definition") => field != Some("name"),
        Some("attribute") => field != Some("attribute"),
        Some("keyword_argument") => field != Some("name"),
        Some("parameters") | Some("lambda_parameters") => false,
        Some("default_parameter") => field != Some("name"),
        Some("typed_parameter") => false,
        Some("typed_default_parameter") => field == Some("value"),
        Some("list_splat_pattern") | Some("dictionary_splat_pattern") => false,
        Some("import_statement") | Some("import_from_statement") | Some("dotted_name")
        | Some("aliased_import") => false,
        Some("global_statement") | Some("nonlocal_statement") => false,
        Some("as_pattern_target") => false,
        _ => true,
    }
}

pub(crate) fn py_halstead(
    node: Node,
    parent_kind: Option<&str>,
    field: Option<&str>,
    parent_bool_op: Option<&str>,
    src: &[u8],
    operators: &mut Halstead,
    operands: &mut Halstead,
) {
    let kind = node.kind();
    let this_bool_op = py_halstead_operator(kind, node, src, parent_bool_op, operators);
    py_halstead_operand(kind, node, parent_kind, field, src, operands);

    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            let child = c.node();
            let child_field = c.field_name();
            py_halstead(child, Some(kind), child_field, this_bool_op, src, operators, operands);
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Count Python Halstead OPERATORS for one node and report the boolean-op context
/// to pass down (chained and/or counts once). Split out of py_halstead.
pub(crate) fn py_halstead_operator(
    kind: &str,
    node: Node,
    src: &[u8],
    parent_bool_op: Option<&str>,
    operators: &mut Halstead,
) -> Option<&'static str> {
    let mut this_bool_op: Option<&str> = None;
    match kind {
        "binary_operator" => {
            if let Some(t) = operator_token(node, src) {
                operators.add(py_binop_name(t));
            }
        }
        "unary_operator" => {
            if let Some(t) = operator_token(node, src) {
                operators.add(py_unary_name(t));
            }
        }
        "not_operator" => operators.add("Not"),
        "boolean_operator" => {
            let tok = operator_token(node, src).unwrap_or("");
            this_bool_op = Some(if tok == "or" { "or" } else { "and" });
            if parent_bool_op != this_bool_op {
                operators.add(if tok == "or" { "Or" } else { "And" });
            }
        }
        "comparison_operator" => {
            for name in py_compare_ops(node, src) {
                operators.add(name);
            }
        }
        "augmented_assignment" => {
            if let Some(t) = operator_token(node, src) {
                operators.add(py_aug_name(t));
            }
        }
        _ => {}
    }
    this_bool_op
}

/// Count Python Halstead OPERANDS (identifiers used as values, literals) for one
/// node. Split out of py_halstead; operand and operator node-kinds are disjoint.
pub(crate) fn py_halstead_operand(
    kind: &str,
    node: Node,
    parent_kind: Option<&str>,
    field: Option<&str>,
    src: &[u8],
    operands: &mut Halstead,
) {
    match kind {
        "identifier" => {
            if py_identifier_is_operand(parent_kind, field) {
                operands.add(node.utf8_text(src).unwrap_or("").to_string());
            }
        }
        "string_content" => {
            let t = node.utf8_text(src).unwrap_or("");
            operands.add(t.chars().take(50).collect::<String>());
        }
        "integer" | "float" => operands.add(node.utf8_text(src).unwrap_or("").to_string()),
        "true" => operands.add("True"),
        "false" => operands.add("False"),
        "none" => operands.add("None"),
        _ => {}
    }
}

// ---- Generic Halstead (php / ruby / ts — no cross-language parity target) ------

const GENERIC_OPERATOR_TOKENS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "//", "++", "--", "==", "===", "!=", "!==", "<>",
    "<", ">", "<=", ">=", "<=>", "&&", "||", "!", "and", "or", "not", "xor", "&", "|",
    "^", "~", "<<", ">>", "=", "+=", "-=", "*=", "/=", "%=", "**=", "//=", "&=", "|=",
    "^=", "<<=", ">>=", ".=", "??", "?", "=~", "->", "=>", "::",
];

/// Rust's leaf kinds carry names no other grammar in the set uses, EXCEPT
/// `primitive_type`, which tree-sitter-php also emits (for `int`/`string` type
/// hints). That collision is why this list is gated on the language instead of
/// being merged into the shared one: folding it in would silently have moved
/// every PHP file's Halstead volume, and therefore its MI, with nothing in the
/// suite to catch it (the parity lock covers Python only). Gating makes
/// "the other four are untouched" true by construction rather than by luck.
pub(crate) fn rust_is_operand_leaf(kind: &str) -> bool {
    matches!(
        kind,
        "integer_literal"
            | "float_literal"
            | "boolean_literal"
            | "char_literal"
            | "primitive_type"
            | "field_identifier"
            | "self"
            | "super"
            | "crate"
    )
}

pub(crate) fn generic_is_operand_leaf(kind: &str, lang: Lang) -> bool {
    if lang == Lang::Rust && rust_is_operand_leaf(kind) {
        return true;
    }
    matches!(
        kind,
        "identifier"
            | "name"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "type_identifier"
            | "constant"
            | "instance_variable"
            | "class_variable"
            | "global_variable"
            | "simple_symbol"
            | "integer"
            | "float"
            | "number"
            | "string_content"
            | "string_fragment"
            | "true"
            | "false"
            | "null"
            | "nil"
            | "none"
    )
}

pub(crate) fn generic_halstead(
    node: Node,
    src: &[u8],
    lang: Lang,
    operators: &mut Halstead,
    operands: &mut Halstead,
) {
    let kind = node.kind();
    if node.is_named() {
        if node.named_child_count() == 0 && generic_is_operand_leaf(kind, lang) {
            operands.add(node.utf8_text(src).unwrap_or("").chars().take(50).collect::<String>());
        }
    } else if GENERIC_OPERATOR_TOKENS.contains(&kind) {
        operators.add(kind);
    }
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            generic_halstead(c.node(), src, lang, operators, operands);
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
}

pub(crate) fn file_volume(root: Node, src: &[u8], lang: Lang) -> f64 {
    let mut operators = Halstead::default();
    let mut operands = Halstead::default();
    match lang {
        // Python alone has a cross-language parity target (metrics.py), so it
        // alone gets the ast-exact implementation. Rust joins PHP / Ruby / TS on
        // the generic walk: there is no second Rust implementation to agree with,
        // and the generic operator/operand split is the same McCabe-adjacent
        // definition the other three already feed into MI.
        Lang::Python => py_halstead(root, None, None, None, src, &mut operators, &mut operands),
        _ => generic_halstead(root, src, lang, &mut operators, &mut operands),
    }
    volume(operators.unique.len(), operands.unique.len(), operators.total, operands.total)
}
