// Native, language-agnostic code-metrics engine (ADR-0002).
//
// Scans SOURCE directly — per file LOC, cyclomatic complexity (McCabe),
// maintainability index (Radon/Microsoft), efferent coupling, function count —
// for Python / PHP / Ruby / TypeScript+JS, with NO Tina4 project and NO running
// framework required. Replaces the four per-framework metrics modules and, for
// the first time, covers the frontend (tina4-js .ts) and arbitrary non-framework
// code.
//
// tina4: ADR-0002 — formulas + thresholds mirror the Python master reference
// tina4-python/tina4_python/dev_admin/metrics.py EXACTLY, so the existing
// `--fail-on` gate thresholds carry over unchanged. The parity is locked by
// `parity_matches_python_master` below, which invokes the real metrics.py.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tree_sitter::{Node, Parser};

// ── Languages ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lang {
    Python,
    Php,
    Ruby,
    Ts, // TypeScript + TSX + JavaScript (parsed with the tsx grammar)
}

impl Lang {
    pub(crate) fn from_path(path: &Path) -> Option<Lang> {
        match path.extension().and_then(|e| e.to_str())?.to_ascii_lowercase().as_str() {
            "py" | "pyw" => Some(Lang::Python),
            "php" => Some(Lang::Php),
            "rb" => Some(Lang::Ruby),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Some(Lang::Ts),
            _ => None,
        }
    }

    fn tree_sitter_language(self) -> tree_sitter::Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Lang::Ts => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

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
    pub has_tests: bool,

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
}

// ── Rounding + MI formula (mirror metrics.py exactly) ─────────────────────────

fn round_dp(value: f64, dp: i32) -> f64 {
    let m = 10f64.powi(dp);
    (value * m).round() / m
}

/// MI = max(0, min(100, (171 - 5.2*ln(V) - 0.23*CC - 16.2*ln(LOC)) * 100/171)).
/// `V` is Halstead volume, `avg_cc` the mean per-function complexity, `loc` the
/// code-line count. Identical to `_maintainability_index` in metrics.py.
fn maintainability_index(volume: f64, avg_cc: f64, loc: usize) -> f64 {
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

fn is_code_line(line: &str, lang: Lang) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    match lang {
        Lang::Python | Lang::Ruby => !t.starts_with('#'),
        Lang::Php => !(t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*')),
        Lang::Ts => !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')),
    }
}

fn count_loc(source: &str, lang: Lang) -> usize {
    source.lines().filter(|l| is_code_line(l, lang)).count()
}

// ── Halstead volume ───────────────────────────────────────────────────────────

#[derive(Default)]
struct Halstead {
    total: usize,
    unique: HashSet<String>,
}
impl Halstead {
    fn add(&mut self, key: impl Into<String>) {
        self.total += 1;
        self.unique.insert(key.into());
    }
}

fn operator_token<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if !child.is_named() {
            return child.utf8_text(src).ok();
        }
    }
    None
}

fn volume(n1: usize, n2: usize, big_n1: usize, big_n2: usize) -> f64 {
    let vocabulary = n1 + n2;
    let length = big_n1 + big_n2;
    if vocabulary > 0 {
        (length as f64) * (vocabulary as f64).log2()
    } else {
        0.0
    }
}

// ---- Python-precise Halstead (mirrors ast.Name / ast.Constant + operator set) --

fn py_binop_name(tok: &str) -> &'static str {
    match tok {
        "+" => "Add", "-" => "Sub", "*" => "Mult", "/" => "Div", "%" => "Mod",
        "**" => "Pow", "//" => "FloorDiv", "<<" => "LShift", ">>" => "RShift",
        "&" => "BitAnd", "|" => "BitOr", "^" => "BitXor", "@" => "MatMult",
        _ => "BinOp",
    }
}

fn py_unary_name(tok: &str) -> &'static str {
    match tok {
        "-" => "USub", "+" => "UAdd", "~" => "Invert", _ => "UnaryOp",
    }
}

fn py_aug_name(tok: &str) -> &'static str {
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
fn py_compare_ops(node: Node, src: &[u8]) -> Vec<&'static str> {
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
fn py_identifier_is_operand(parent_kind: Option<&str>, field: Option<&str>) -> bool {
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

fn py_halstead(
    node: Node,
    parent_kind: Option<&str>,
    field: Option<&str>,
    parent_bool_op: Option<&str>,
    src: &[u8],
    operators: &mut Halstead,
    operands: &mut Halstead,
) {
    let kind = node.kind();
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

// ---- Generic Halstead (php / ruby / ts — no cross-language parity target) ------

const GENERIC_OPERATOR_TOKENS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "//", "++", "--", "==", "===", "!=", "!==", "<>",
    "<", ">", "<=", ">=", "<=>", "&&", "||", "!", "and", "or", "not", "xor", "&", "|",
    "^", "~", "<<", ">>", "=", "+=", "-=", "*=", "/=", "%=", "**=", "//=", "&=", "|=",
    "^=", "<<=", ">>=", ".=", "??", "?", "=~", "->", "=>", "::",
];

fn generic_is_operand_leaf(kind: &str) -> bool {
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

fn generic_halstead(node: Node, src: &[u8], operators: &mut Halstead, operands: &mut Halstead) {
    let kind = node.kind();
    if node.is_named() {
        if node.named_child_count() == 0 && generic_is_operand_leaf(kind) {
            operands.add(node.utf8_text(src).unwrap_or("").chars().take(50).collect::<String>());
        }
    } else if GENERIC_OPERATOR_TOKENS.contains(&kind) {
        operators.add(kind);
    }
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            generic_halstead(c.node(), src, operators, operands);
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
}

fn file_volume(root: Node, src: &[u8], lang: Lang) -> f64 {
    let mut operators = Halstead::default();
    let mut operands = Halstead::default();
    match lang {
        Lang::Python => py_halstead(root, None, None, None, src, &mut operators, &mut operands),
        _ => generic_halstead(root, src, &mut operators, &mut operands),
    }
    volume(operators.unique.len(), operands.unique.len(), operators.total, operands.total)
}

// ── Cyclomatic complexity ─────────────────────────────────────────────────────
// CC = 1 + decision points, counted over the function's ENTIRE subtree (nested
// functions included — matching ast.walk in metrics.py). Python's decision set is
// replicated exactly; the other languages use the same McCabe definition applied
// through their grammar (if/loop/case/catch/ternary/&&/||).

fn is_boolean_binary(node: Node, src: &[u8]) -> bool {
    matches!(operator_token(node, src), Some("&&") | Some("||") | Some("and") | Some("or"))
}

fn is_decision(node: Node, lang: Lang, src: &[u8]) -> u32 {
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
    }
}

/// True for a node that opens a new measurement scope: a nested function (it is
/// reported as a function in its own right) or a class body (its methods are).
/// Mirrors metrics.py, which skips FunctionDef / AsyncFunctionDef / ClassDef.
fn is_scope_boundary(kind: &str, lang: Lang) -> bool {
    if is_function_node(kind, lang) {
        return true;
    }
    match lang {
        Lang::Python => kind == "class_definition",
        Lang::Php => matches!(kind, "class_declaration" | "interface_declaration"
            | "trait_declaration" | "enum_declaration"),
        Lang::Ruby => matches!(kind, "class" | "module"),
        Lang::Ts => matches!(kind, "class_declaration" | "class"),
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
/// A Python lambda is NOT a scope boundary - lambdas are never listed as
/// functions, so their decisions would vanish. TypeScript arrow functions ARE
/// listed, so they are boundaries.
fn count_own_decisions(node: Node, lang: Lang, src: &[u8]) -> u32 {
    let mut total = is_decision(node, lang, src);
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            let child = c.node();
            if !is_scope_boundary(child.kind(), lang) {
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

fn is_function_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => kind == "function_definition",
        Lang::Php => matches!(kind, "function_definition" | "method_declaration"),
        Lang::Ruby => matches!(kind, "method" | "singleton_method"),
        // tina4-js is arrow-function heavy; count them so their complexity is
        // attributed (Python excludes only trivial one-line lambdas — arrows are
        // full function bodies, so they earn a slot).
        Lang::Ts => matches!(
            kind,
            "function_declaration"
                | "generator_function_declaration"
                | "method_definition"
                | "function_expression"
                | "arrow_function"
        ),
    }
}

fn is_class_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => kind == "class_definition",
        Lang::Php => matches!(kind, "class_declaration" | "trait_declaration" | "interface_declaration"),
        Lang::Ruby => matches!(kind, "class" | "module"),
        Lang::Ts => matches!(kind, "class_declaration" | "class"),
    }
}

fn node_name(node: Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
}

/// Outermost enclosing class name (matches metrics.py `_get_parent_class`, which
/// returns the BFS-first — i.e. outermost — containing class).
fn outer_class_name(node: Node, lang: Lang, src: &[u8]) -> Option<String> {
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

fn function_display_name(node: Node, lang: Lang, src: &[u8]) -> String {
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

fn collect_functions<'a>(node: Node<'a>, lang: Lang, src: &[u8], out: &mut Vec<(Node<'a>, u32)>) {
    if is_function_node(node.kind(), lang) {
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

// ── Coupling (efferent) ───────────────────────────────────────────────────────

fn is_import_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => matches!(kind, "import_statement" | "import_from_statement"),
        Lang::Php => matches!(kind, "namespace_use_declaration" | "require_once_expression"
            | "require_expression" | "include_expression" | "include_once_expression"),
        Lang::Ruby => false, // require is a plain method call — counted below
        Lang::Ts => matches!(kind, "import_statement"),
    }
}

/// Strip one layer of quoting from a string-literal node's text.
fn unquote(raw: &str) -> String {
    let t = raw.trim();
    let t = t.strip_prefix("b").unwrap_or(t); // php b"" / rb byte-ish prefixes
    for q in ['"', '\'', '`'] {
        if let Some(inner) = t.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return inner.to_string();
        }
    }
    t.to_string()
}

/// The first string literal anywhere under `node` (import/require argument).
fn first_string_literal(node: Node, src: &[u8]) -> Option<String> {
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind().contains("string") && n.child_count() == 0 || n.kind() == "string" {
            if let Ok(t) = n.utf8_text(src) {
                let u = unquote(t);
                if !u.is_empty() {
                    return Some(u);
                }
            }
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    None
}

/// Every import SPECIFIER the file writes, as written in the source: a Python
/// dotted module, a TS module specifier, a Ruby require argument, a PHP `use`
/// path or include target. Resolution to a file happens later, in
/// `analyze_targets`, because it needs to know every file in the scan.
fn extract_import_specs(root: Node, lang: Lang, src: &[u8]) -> Vec<String> {
    let mut specs: Vec<String> = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        match lang {
            Lang::Python if kind == "import_from_statement" => {
                // `from X import y` -> X (may be relative: `.`, `..pkg`)
                if let Some(m) = node.child_by_field_name("module_name") {
                    if let Ok(t) = m.utf8_text(src) {
                        specs.push(t.trim().to_string());
                    }
                } else if let Ok(t) = node.utf8_text(src) {
                    // bare `from . import x` — module_name absent in some grammars
                    if let Some(rest) = t.trim().strip_prefix("from ") {
                        let dots: String =
                            rest.chars().take_while(|c| *c == '.').collect();
                        if !dots.is_empty() {
                            specs.push(dots);
                        }
                    }
                }
            }
            Lang::Python if kind == "import_statement" => {
                // `import a.b, c` -> each dotted_name
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    match child.kind() {
                        "dotted_name" => {
                            if let Ok(t) = child.utf8_text(src) {
                                specs.push(t.trim().to_string());
                            }
                        }
                        "aliased_import" => {
                            if let Some(n) = child.child_by_field_name("name") {
                                if let Ok(t) = n.utf8_text(src) {
                                    specs.push(t.trim().to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Lang::Ts if kind == "import_statement" => {
                if let Some(s) = node.child_by_field_name("source") {
                    if let Ok(t) = s.utf8_text(src) {
                        specs.push(unquote(t));
                    }
                } else if let Some(s) = first_string_literal(node, src) {
                    specs.push(s);
                }
            }
            // `use Tina4\Frond;` and `use Tina4\{A, B};` nest the path one level
            // down, inside a namespace_use_clause / -group-clause - matching only
            // the outer declaration's DIRECT children found nothing at all, which
            // is why PHP produced zero edges across 138 files.
            Lang::Php
                if matches!(kind, "namespace_use_clause" | "namespace_use_group_clause") =>
            {
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    if child.kind().contains("qualified_name") || child.kind() == "name" {
                        if let Ok(t) = child.utf8_text(src) {
                            let t = t.trim();
                            if !t.is_empty() {
                                specs.push(t.to_string());
                            }
                            break; // the first name is the path; the rest is `as Alias`
                        }
                    }
                }
            }
            Lang::Php
                if matches!(
                    kind,
                    "require_once_expression"
                        | "require_expression"
                        | "include_expression"
                        | "include_once_expression"
                ) =>
            {
                if let Some(s) = first_string_literal(node, src) {
                    specs.push(s);
                }
            }
            // PHP's DOMINANT idiom is not `use` at all - it is an inline
            // fully-qualified reference, `\Tina4\Database::create(...)`, resolved
            // by the autoloader. In the real framework that is 243 references
            // against 72 `use` statements, so counting only `use` understated
            // PHP coupling by roughly 3-4x. A namespaced reference IS a
            // dependency; require a backslash so bare function calls are ignored.
            Lang::Php if kind.contains("qualified_name") => {
                if let Ok(t) = node.utf8_text(src) {
                    let t = t.trim();
                    if t.contains('\\') && t.len() > 1 {
                        specs.push(t.to_string());
                    }
                }
            }
            Lang::Ruby if kind == "call" => {
                if let Some(m) = node
                    .child_by_field_name("method")
                    .and_then(|n| n.utf8_text(src).ok())
                {
                    if matches!(m, "require" | "require_relative" | "load" | "autoload") {
                        if let Some(args) = node.child_by_field_name("arguments") {
                            if let Some(s) = first_string_literal(args, src) {
                                // `autoload :Sym, "path"` -> the path is the literal
                                specs.push(s);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            stack.push(child);
        }
    }
    // Deduplicate: referencing the same module thirty times is ONE dependency,
    // not thirty. Without this, PHP's inline-FQN idiom would inflate dep_count
    // (the dashboard badge) enormously while the edge set stayed correct.
    specs.sort();
    specs.dedup();
    specs
}

/// Normalise a candidate path: drop `./`, collapse `a/../b`, unify separators.
fn normalise_path(raw: &str) -> String {
    let unified = raw.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    for seg in unified.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// The directory part of a relative file path ("a/b/c.py" -> "a/b").
fn parent_dir(rel: &str) -> String {
    match rel.rfind('/') {
        Some(i) => rel[..i].to_string(),
        None => String::new(),
    }
}

/// Resolve one import specifier to a file INSIDE the scanned set, or None when
/// it points outside it (stdlib, a third-party package, a generated asset).
///
/// Only internal edges become coupling, so an unresolvable specifier is not an
/// error - it is simply an external dependency and is excluded by design.
fn resolve_import(
    spec: &str,
    from_rel: &str,
    lang: Lang,
    paths: &HashSet<String>,
    root_pkg: Option<&str>,
) -> Option<String> {
    let dir = parent_dir(from_rel);
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));

    match lang {
        Lang::Python => {
            // Leading dots = relative import; each dot climbs one package level.
            let dots = spec.chars().take_while(|c| *c == '.').count();
            let tail = spec.trim_start_matches('.').replace('.', "/");
            if dots > 0 {
                let mut base = dir.clone();
                for _ in 1..dots {
                    base = parent_dir(&base);
                }
                let joined = if tail.is_empty() {
                    base.clone()
                } else if base.is_empty() {
                    tail.clone()
                } else {
                    format!("{base}/{tail}")
                };
                push(format!("{joined}.py"));
                push(format!("{joined}/__init__.py"));
            } else {
                push(format!("{tail}.py"));
                push(format!("{tail}/__init__.py"));
                // Absolute intra-package import: `tina4_python.debug` while the
                // scan root IS tina4_python, so the package prefix is implicit.
                if let Some(pkg) = root_pkg {
                    if let Some(rest) = tail.strip_prefix(&format!("{pkg}/")) {
                        push(format!("{rest}.py"));
                        push(format!("{rest}/__init__.py"));
                    }
                }
            }
        }
        Lang::Ts => {
            // Only a relative specifier can be internal; bare ones are packages.
            if !(spec.starts_with('.') || spec.starts_with('/')) {
                return None;
            }
            let joined = if dir.is_empty() {
                spec.to_string()
            } else {
                format!("{dir}/{spec}")
            };
            let base = normalise_path(&joined);
            // TS source may import a ".js" path that is really the ".ts" file.
            let stem = base
                .strip_suffix(".js")
                .or_else(|| base.strip_suffix(".mjs"))
                .unwrap_or(&base)
                .to_string();
            for ext in ["ts", "tsx", "js", "mjs", "jsx"] {
                push(format!("{stem}.{ext}"));
                push(format!("{stem}/index.{ext}"));
            }
            push(base.clone());
        }
        Lang::Ruby => {
            let cleaned = spec.trim_end_matches(".rb");
            // require_relative is dir-relative; require is load-path-relative.
            let joined = if dir.is_empty() {
                cleaned.to_string()
            } else {
                format!("{dir}/{cleaned}")
            };
            push(format!("{joined}.rb"));
            push(format!("{cleaned}.rb"));
            push(format!("lib/{cleaned}.rb"));
            if let Some(pkg) = root_pkg {
                if let Some(rest) = cleaned.strip_prefix(&format!("{pkg}/")) {
                    push(format!("{rest}.rb"));
                }
            }
        }
        Lang::Php => {
            // A `use` path is a namespace; PSR-4 maps it onto directories.
            let as_path = spec.trim_start_matches('\\').replace('\\', "/");
            push(format!("{as_path}.php"));
            if let Some(pkg) = root_pkg {
                if let Some(rest) = as_path.strip_prefix(&format!("{pkg}/")) {
                    push(format!("{rest}.php"));
                }
            }
            // Drop the vendor-style first segment (Tina4\Frond -> Frond.php).
            if let Some((_, rest)) = as_path.split_once('/') {
                push(format!("{rest}.php"));
            }
            // require/include of a literal path, possibly dir-relative.
            if !dir.is_empty() {
                push(format!("{dir}/{as_path}"));
            }
            push(as_path.clone());
        }
    }

    candidates
        .into_iter()
        .find(|c| !c.is_empty() && c != from_rel && paths.contains(c))
}

// ── Analyze one source string ─────────────────────────────────────────────────

pub(crate) fn analyze_source(
    lang: Lang,
    source: &str,
    rel_path: &str,
    has_tests: bool,
) -> Option<(FileMetrics, Vec<FunctionInfo>, Vec<String>)> {
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language()).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let src = source.as_bytes();

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
        has_tests,
        // dep_count is every import as written; the coupling triple is filled in
        // by the second pass in analyze_targets, which alone knows every file.
        dep_count: specs.len(),
        coupling_efferent: 0,
        coupling_afferent: 0,
        instability: 0.0,
    };
    Some((fm, functions, specs))
}

// ── File discovery ─────────────────────────────────────────────────────────────

const IGNORED_DIRS: &[&str] = &[
    "node_modules", "vendor", ".git", "target", "dist", "build", "__pycache__",
    ".venv", "venv", "coverage", ".next", "out", ".tina4-docs", ".idea", ".pytest_cache",
    ".mypy_cache", ".ruff_cache", "site-packages",
];

/// Build output, not source: a bundled/minified asset that no human maintains.
///
/// This matters because the engine is language-agnostic and therefore sees `.js`
/// that the retired per-language modules never did. A single minified line scores
/// an absurd complexity (26,416 on one real bundle) and would otherwise take over
/// the offenders list and `--fail-on`, burying the code someone can actually fix.
fn is_generated_asset(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".min.js")
        || name.ends_with(".min.ts")
        || name.ends_with(".min.css")
        || name.ends_with(".bundle.js")
        || name.ends_with("-min.js")
        || name.ends_with(".map")
}

/// Minified-by-content catch-all for a bundle that is not named like one.
/// A hand-written source file does not average hundreds of characters per line.
fn looks_minified(source: &str) -> bool {
    let lines = source.lines().count();
    if lines == 0 {
        return false;
    }
    // A single very long line is the classic minified shape.
    source.len() / lines > 200
}

fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut items: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    items.sort();
    for path in items {
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || IGNORED_DIRS.contains(&name) {
                continue;
            }
            walk_dir(&path, files);
        } else if Lang::from_path(&path).is_some() && !is_generated_asset(&path) {
            files.push(path);
        }
    }
}

/// Resolve the scan root(s). With `--path` honour it (file or dir). Otherwise
/// default to cwd auto-detecting `src/`, then `packages/*/src`, then `.`.
fn resolve_targets(path_flag: Option<&str>) -> Result<(Vec<PathBuf>, String), String> {
    let mut files = Vec::new();
    if let Some(p) = path_flag {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            if Lang::from_path(&pb).is_none() {
                return Err(format!("unsupported file type: {p}"));
            }
            return Ok((vec![pb], p.to_string()));
        }
        if pb.is_dir() {
            walk_dir(&pb, &mut files);
            return Ok((files, p.to_string()));
        }
        return Err(format!("Directory not found: {p}"));
    }

    let src = PathBuf::from("src");
    if src.is_dir() {
        walk_dir(&src, &mut files);
        if !files.is_empty() {
            return Ok((files, "src".to_string()));
        }
    }
    let packages = PathBuf::from("packages");
    if packages.is_dir() {
        if let Ok(entries) = fs::read_dir(&packages) {
            let mut pkg_dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            pkg_dirs.sort();
            for pkg in pkg_dirs {
                let pkg_src = pkg.join("src");
                if pkg_src.is_dir() {
                    walk_dir(&pkg_src, &mut files);
                }
            }
        }
        if !files.is_empty() {
            return Ok((files, "packages/*/src".to_string()));
        }
    }
    walk_dir(Path::new("."), &mut files);
    Ok((files, ".".to_string()))
}

fn rel_display(file: &Path, root: &str) -> String {
    let root_path = Path::new(root);
    let rel = if root_path.is_file() {
        file.file_name().map(PathBuf::from).unwrap_or_else(|| file.to_path_buf())
    } else {
        file.strip_prefix(root_path).unwrap_or(file).to_path_buf()
    };
    rel.to_string_lossy().replace('\\', "/")
}

// ── Test detection (pragmatic; drives only the `info untested` offender) ────────
// A best-effort, language-agnostic reimplementation — NOT a byte-exact port of
// metrics.py's Python-dotted-path heuristic. It never affects `--fail-on`
// (untested is `info`). Signals: a dedicated test file named for the module, or a
// test file that mentions the module stem on an import/require/use line.

struct TestIndex {
    file_names: HashSet<String>,
    contents: Vec<String>,
}

fn build_test_index(root: &str) -> TestIndex {
    let mut file_names = HashSet::new();
    let mut contents = Vec::new();
    let base = {
        let p = Path::new(root);
        if p.is_file() { p.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")) } else { p.to_path_buf() }
    };
    // Search cwd and up to 5 ancestors of the scan root for test dirs.
    let mut roots: Vec<PathBuf> = vec![PathBuf::from(".")];
    let mut cur = base.clone();
    for _ in 0..6 {
        roots.push(cur.clone());
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => break,
        }
    }
    for r in roots {
        for td in ["tests", "test", "spec"] {
            let dir = r.join(td);
            if dir.is_dir() {
                let mut tf = Vec::new();
                walk_dir(&dir, &mut tf);
                for f in tf {
                    if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
                        file_names.insert(name.to_ascii_lowercase());
                    }
                    if let Ok(c) = fs::read_to_string(&f) {
                        contents.push(c);
                    }
                }
            }
        }
    }
    TestIndex { file_names, contents }
}

/// Type names DECLARED in this module (class / trait / interface / module).
///
/// Used by the class-symbol stage of test detection: a test that imports a class
/// through the package root never mentions the module's file stem, so the stem
/// signal alone reports a well-tested module as untested.
fn declared_type_names(source: &str, lang: Lang) -> Vec<String> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter_language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else { return Vec::new() };
    let bytes = source.as_bytes();
    let mut names = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if is_class_node(node.kind(), lang) {
            if let Some(name) = node_name(node, bytes) {
                names.push(name);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    names
}

/// True when `needle` appears in `haystack` as a whole identifier.
///
/// A substring match would let `Order` mark `OrderItem` as tested, and every
/// 3-char name would collide constantly. Identifier characters on either side
/// disqualify the hit.
fn mentions_symbol(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let n = needle.len();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0usize;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + n;
        let before_ok = start == 0 || !is_ident(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn module_has_tests(file: &Path, idx: &TestIndex, declared_types: &[String]) -> bool {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let stem = if matches!(stem, "__init__" | "index" | "mod") {
        file.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or(stem)
    } else {
        stem
    };
    if stem.is_empty() {
        return false;
    }
    let stem_l = stem.to_ascii_lowercase();
    // Stage 1: a dedicated test file named for this module.
    //
    // `{stem}test.` covers PHPUnit's PascalCase convention (Metrics.php ->
    // MetricsTest.php), which every other pattern here misses because it uses no
    // separator. Stage 3 cannot rescue it either: whole-identifier matching
    // correctly refuses to find `Widget` inside `WidgetTest`. Comparison is
    // lowercased and anchored with starts_with, so `Base` never matches
    // `DatabaseTest.php`.
    for pat in [
        format!("test_{stem_l}."),
        format!("test_{stem_l}s."),
        format!("{stem_l}_test."),
        format!("{stem_l}_spec."),
        format!("{stem_l}.test."),
        format!("{stem_l}.spec."),
        format!("{stem_l}test."),
    ] {
        if idx.file_names.iter().any(|n| n.starts_with(&pat)) {
            return true;
        }
    }
    // Stage 2: an import/require/use line that mentions the module stem.
    for content in &idx.contents {
        for line in content.lines() {
            let t = line.trim_start();
            let is_import = t.starts_with("import ")
                || t.starts_with("from ")
                || t.starts_with("require")
                || t.starts_with("use ")
                || t.contains("require(");
            if is_import && line.contains(stem) {
                return true;
            }
        }
    }
    // Stage 3: a TYPE DECLARED here is referenced by a test.
    //
    // A test that imports through the package root (`from src import ORM`) never
    // mentions the module's file stem, so stages 1 and 2 both miss it and a
    // well-tested module is reported untested. The class symbol is the only
    // signal in that case, and it is a real one.
    //
    // No minimum name length. A >3-char gate was the bug the Python master
    // fixed: it silently excluded exactly the short framework types that matter
    // most (ORM, Api, Log). Whole-identifier matching is what keeps a short name
    // honest, not a length floor.
    for ty in declared_types {
        for content in &idx.contents {
            if mentions_symbol(content, ty) {
                return true;
            }
        }
    }
    false
}

// ── Offenders ───────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub(crate) struct Offender {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub severity: String,
    pub score: f64,
    pub detail: String,
}

fn severity_rank(sev: &str) -> u8 {
    match sev {
        "error" => 2,
        "warn" => 1,
        _ => 0,
    }
}

/// Build the ranked offender list, mirroring `offenders()` in metrics.py: one
/// rule each for function complexity, large_file, too_many_functions,
/// low_maintainability, untested.
fn build_offenders(files: &[FileMetrics], functions: &[FunctionInfo]) -> Vec<Offender> {
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
            items.push(Offender {
                file: fn_info.file.clone(),
                line: fn_info.line,
                kind: "complexity".to_string(),
                severity: if cc > 20 { "error" } else { "warn" }.to_string(),
                score: cc as f64,
                detail: format!("{} \u{2014} cyclomatic complexity {}", fn_info.name, cc),
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
            });
        }
        if fm.maintainability < 40.0 {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "low_maintainability".to_string(),
                severity: if fm.maintainability < 20.0 { "error" } else { "warn" }.to_string(),
                score: 50.0 - fm.maintainability,
                detail: format!("maintainability index {:.1} (min 40)", fm.maintainability),
            });
        }
        if !fm.has_tests {
            items.push(Offender {
                file: fm.path.clone(),
                line: 1,
                kind: "untested".to_string(),
                severity: "info".to_string(),
                score: fm.loc as f64 / 100.0,
                detail: "no referencing test".to_string(),
            });
        }
    }

    // Sort by (severity rank, score) descending, stable.
    items.sort_by(|a, b| {
        severity_rank(&b.severity)
            .cmp(&severity_rank(&a.severity))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    items
}

// ── Summary + JSON payload ────────────────────────────────────────────────────

#[derive(Serialize)]
struct Summary {
    files_analyzed: usize,
    total_functions: usize,
    avg_complexity: f64,
    avg_maintainability: f64,
    scan_mode: String,
    scan_root: String,
    total_offenders: usize,
}

#[derive(Serialize)]
struct JsonPayload {
    summary: Summary,
    offenders: Vec<Offender>,
    /// Per-file rows. The dev dashboard draws ONE BUBBLE PER FILE from these,
    /// sizing by `loc`, colouring by `avg_complexity` and badging `dep_count`,
    /// so this section is what keeps the metrics visualisation alive when the
    /// per-framework metrics modules are retired (ADR-0002).
    file_metrics: Vec<FileMetrics>,
    /// Top 15 by complexity, for the "most complex functions" table.
    most_complex_functions: Vec<FunctionInfo>,
    /// file -> imported files, both sides relative paths inside the scan.
    dependency_graph: BTreeMap<String, Vec<String>>,
}

pub(crate) struct Report {
    files: Vec<FileMetrics>,
    functions: Vec<FunctionInfo>,
    offenders: Vec<Offender>,
    scan_root: String,
    /// file -> the files it imports, both sides RELATIVE PATHS in the scanned
    /// set. Keys and values live in the same namespace on purpose: the previous
    /// implementation keyed this on module names while looking it up by path, so
    /// nothing ever matched and the dependency view drew no edges at all.
    dependency_graph: BTreeMap<String, Vec<String>>,
}

pub(crate) fn analyze_targets(files: &[PathBuf], scan_root: &str) -> Report {
    let test_index = build_test_index(scan_root);
    let mut file_metrics: Vec<FileMetrics> = Vec::new();
    let mut all_functions: Vec<FunctionInfo> = Vec::new();
    // Pass 1 cannot resolve imports: a specifier only becomes an edge once every
    // file in the scan is known. Park the raw specifiers with their language.
    let mut pending: Vec<(String, Lang, Vec<String>)> = Vec::new();

    for path in files {
        let Some(lang) = Lang::from_path(path) else { continue };
        let Ok(source) = fs::read_to_string(path) else { continue };
        // A bundle that is not NAMED like one still must not skew the report.
        if looks_minified(&source) {
            continue;
        }
        let rel = rel_display(path, scan_root);
        let declared = declared_type_names(&source, lang);
        let has_tests = module_has_tests(path, &test_index, &declared);
        if let Some((fm, funcs, specs)) = analyze_source(lang, &source, &rel, has_tests) {
            pending.push((fm.path.clone(), lang, specs));
            file_metrics.push(fm);
            all_functions.extend(funcs);
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

    let offenders = build_offenders(&file_metrics, &all_functions);
    Report {
        files: file_metrics,
        functions: all_functions,
        offenders,
        scan_root: scan_root.to_string(),
        dependency_graph,
    }
}

fn build_summary(report: &Report, total_offenders: usize) -> Summary {
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
    }
}

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

/// `tina4 metrics` — native, language-agnostic. Returns the process exit code.
pub fn run(path: Option<String>, top: Option<usize>, json: bool, fail_on: Option<String>) -> i32 {
    if let Some(f) = &fail_on {
        if f != "warn" && f != "error" {
            eprintln!("  invalid --fail-on '{f}' (use warn or error)");
            return 2;
        }
    }
    let top = top.unwrap_or(20);

    let (files, scan_root) = match resolve_targets(path.as_deref()) {
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

    // Exit code from the FULL offender set (before top truncation).
    let has_warn = report.offenders.iter().any(|o| o.severity == "warn");
    let has_error = report.offenders.iter().any(|o| o.severity == "error");
    let exit_code = compute_exit_code(fail_on.as_deref(), has_warn, has_error);

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
        };
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()));
        return exit_code;
    }

    print_human(&summary, &shown);
    exit_code
}

fn print_human(summary: &Summary, shown: &[Offender]) {
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
    let showing = if shown.is_empty() { String::new() } else { format!(" (showing top {})", shown.len()) };
    println!("  offenders: {} total{}", summary.total_offenders, showing);
    println!();

    if shown.is_empty() {
        println!("  {}", paint("\u{2713} no offenders \u{2014} clean", "32"));
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
    }
    println!();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn read_fixture(name: &str) -> String {
        std::fs::read_to_string(manifest().join("tests/fixtures").join(name)).unwrap()
    }

    fn analyze_py(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = analyze_source(Lang::Python, src, "t.py", false).unwrap();
        (fm, fns)
    }

    fn analyze_ts(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = analyze_source(Lang::Ts, src, "t.ts", false).unwrap();
        (fm, fns)
    }

    fn analyze_php(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = analyze_source(Lang::Php, src, "t.php", false).unwrap();
        (fm, fns)
    }

    fn analyze_rb(src: &str) -> (FileMetrics, Vec<FunctionInfo>) {
        let (fm, fns, _specs) = analyze_source(Lang::Ruby, src, "t.rb", false).unwrap();
        (fm, fns)
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
        assert_eq!(Lang::from_path(Path::new("a.md")), None);
        assert_eq!(Lang::from_path(Path::new("noext")), None);
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
    fn a_python_lambda_still_counts_toward_its_enclosing_function() {
        // Lambdas are never listed as functions of their own, so excluding them
        // would silently lose their decisions instead of relocating them.
        let src = "def f(xs):\n    return sorted(xs, key=lambda x: 1 if x else 0)\n";
        let (_fm, fns) = analyze_py(src);
        assert_eq!(fns.len(), 1, "the lambda is not reported separately");
        assert_eq!(fns[0].complexity, 2, "1 + the lambda's ternary");
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
    fn a_php_closure_counts_toward_its_enclosing_method() {
        // Anonymous functions are not listed as functions of their own (same rule
        // as a Python lambda), so their decisions stay where they are written
        // rather than disappearing.
        let php = "<?php\nclass A {\n  function outer($x) {\n    return array_map(function ($y) { if ($y) { return 1; } return 2; }, $x);\n  }\n}\n";
        let (_fm, fns) = analyze_php(php);
        let outer = fns.iter().find(|f| f.name.ends_with("outer")).unwrap();
        assert_eq!(outer.complexity, 2, "1 + the closure's if");
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
        FileMetrics {
            path: "x.py".into(), loc, complexity: 0, avg_complexity: 0.0,
            functions: funcs, maintainability: mi, has_tests,
            dep_count: 0, coupling_efferent: 0, coupling_afferent: 0, instability: 0.0,
        }
    }
    fn func_with(cc: u32) -> FunctionInfo {
        FunctionInfo { name: "f".into(), file: "x.py".into(), line: 1, complexity: cc, loc: 1 }
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
    fn offender_complexity_severity_split_at_20() {
        let warn = build_offenders(&[], &[func_with(15)]);
        assert_eq!(warn[0].kind, "complexity");
        assert_eq!(warn[0].severity, "warn");
        let err = build_offenders(&[], &[func_with(25)]);
        assert_eq!(err[0].severity, "error");
        // CC <= 10 is not an offender.
        assert!(build_offenders(&[], &[func_with(10)]).is_empty());
    }

    #[test]
    fn offender_too_many_functions_and_untested() {
        let offs = build_offenders(&[file_with(90.0, 10, 21, false)], &[]);
        assert!(offs.iter().any(|o| o.kind == "too_many_functions" && o.severity == "warn"));
        assert!(offs.iter().any(|o| o.kind == "untested" && o.severity == "info"));
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

    // ---- Non-Python: proves no framework / no project needed -----------------

    #[test]
    fn analyzes_php_ruby_typescript_without_a_project() {
        let php = "<?php\nfunction f($a){ if ($a && $a > 0) { return 1; } return 0; }\n";
        let (fm, fns, _s) = analyze_source(Lang::Php, php, "t.php", false).unwrap();
        assert_eq!(fm.functions, 1);
        assert!(fns[0].complexity >= 3); // 1 + if + &&
        assert!(fm.maintainability > 0.0 && fm.maintainability <= 100.0);

        let rb = "def f(a)\n  return 1 if a && a > 0\n  0\nend\n";
        let (fm, _f, _s) = analyze_source(Lang::Ruby, rb, "t.rb", false).unwrap();
        assert_eq!(fm.functions, 1);

        let ts = "export const f = (a: number) => { if (a && a > 0) { return 1; } return 0; };\n";
        let (fm, fns, _s) = analyze_source(Lang::Ts, ts, "t.ts", false).unwrap();
        assert_eq!(fm.functions, 1, "top-level arrow function is counted");
        assert!(fns[0].complexity >= 3);
    }

    #[test]
    fn typescript_imports_are_counted_as_dep_count() {
        let ts = "import { a } from './a';\nimport b from './b';\nconst x = () => a + b;\n";
        let (fm, _f, specs) = analyze_source(Lang::Ts, ts, "t.ts", false).unwrap();
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
        let (fm, _f, specs) = analyze_source(Lang::Php, php, "App.php", false).unwrap();
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
        let (fm, _f, specs) = analyze_source(Lang::Php, php, "Frond.php", false).unwrap();
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

    // ---- The parity lock against the REAL Python master reference ------------

    fn locate_tina4_python() -> Option<PathBuf> {
        if let Ok(d) = std::env::var("TINA4_PYTHON_DIR") {
            let p = PathBuf::from(d);
            if p.is_dir() {
                return Some(p);
            }
        }
        let candidate = manifest().join("../tina4-python");
        if candidate.join("tina4_python/dev_admin/metrics.py").is_file() {
            return Some(candidate);
        }
        None
    }

    fn python_bin(dir: &Path) -> Option<PathBuf> {
        let venv = dir.join(".venv/bin/python");
        if venv.is_file() {
            return Some(venv);
        }
        for name in ["python3", "python"] {
            if let Ok(p) = which::which(name) {
                return Some(p);
            }
        }
        None
    }

    #[derive(serde::Deserialize)]
    struct PyRef {
        loc: usize,
        complexity: u32,
        functions: usize,
        maintainability: f64,
        avg_complexity: f64,
        #[serde(default)]
        error: Option<String>,
    }

    /// No mocks: shells out to the REAL tina4-python metrics.py and asserts the
    /// Rust engine lands on the same numbers for the same real source files.
    #[test]
    fn parity_matches_python_master() {
        let Some(dir) = locate_tina4_python() else {
            eprintln!("SKIP parity: tina4-python not found (set TINA4_PYTHON_DIR)");
            return;
        };
        let Some(py) = python_bin(&dir) else {
            eprintln!("SKIP parity: no python interpreter available");
            return;
        };
        let driver = manifest().join("tests/parity_reference.py");

        for name in ["sample_container.py", "sample_metrics.py"] {
            let fixture = manifest().join("tests/fixtures").join(name);
            let out = Command::new(&py)
                .arg(&driver)
                .arg(&fixture)
                .env("TINA4_PYTHON_DIR", &dir)
                .output()
                .expect("failed to run parity_reference.py");
            let stdout = String::from_utf8_lossy(&out.stdout);
            let reference: PyRef = serde_json::from_str(stdout.trim())
                .unwrap_or_else(|_| panic!("bad driver output for {name}: {stdout}"));
            if let Some(err) = &reference.error {
                eprintln!("SKIP parity for {name}: {err}");
                return;
            }

            let src = read_fixture(name);
            let (fm, _fns) = analyze_py(&src);

            eprintln!(
                "PARITY {name}: py(loc={},cc={},fn={},mi={},avg={}) rust(loc={},cc={},fn={},mi={},avg={})",
                reference.loc, reference.complexity, reference.functions,
                reference.maintainability, reference.avg_complexity,
                fm.loc, fm.complexity, fm.functions, fm.maintainability, fm.avg_complexity
            );

            assert_eq!(fm.loc, reference.loc, "{name}: LOC must match exactly");
            assert_eq!(fm.complexity, reference.complexity, "{name}: total CC must match exactly");
            assert_eq!(fm.functions, reference.functions, "{name}: function count must match exactly");
            assert!(
                (fm.avg_complexity - reference.avg_complexity).abs() <= 0.01,
                "{name}: avg complexity {} vs {}", fm.avg_complexity, reference.avg_complexity
            );
            // MI matched EXACTLY to the reported 0.1 in development; the 0.15
            // tolerance is a guard against float-rounding drift across platforms.
            assert!(
                (fm.maintainability - reference.maintainability).abs() <= 0.15,
                "{name}: MI {} vs {}", fm.maintainability, reference.maintainability
            );
        }
    }

    // ── Test detection: the class-symbol stage ──────────────────────────
    //
    // A test that imports through the package root never mentions the module's
    // file stem, so stages 1 and 2 both miss it. Before this stage existed,
    // `tina4 metrics` reported a well-tested module as untested and raised an
    // "untested" offender for it.

    #[test]
    fn declared_type_names_finds_a_short_class() {
        let names = declared_type_names("class ORM:\n    def save(self):\n        return True\n", Lang::Python);
        assert!(names.contains(&"ORM".to_string()), "got {names:?}");
    }

    #[test]
    fn a_three_char_class_referenced_by_a_test_counts_as_tested() {
        // No length floor. A >3-char gate was the bug the Python master fixed:
        // it excluded exactly the short framework types that matter (ORM, Api, Log).
        let idx = TestIndex {
            file_names: ["test_models.py".to_string()].into_iter().collect(),
            contents: vec!["from src import ORM\n\ndef test_save():\n    assert ORM().save()\n".to_string()],
        };
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
        let idx = TestIndex {
            file_names: ["test_other.py".to_string()].into_iter().collect(),
            contents: vec!["def test_nothing():\n    assert True\n".to_string()],
        };
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
    fn a_phpunit_pascalcase_test_file_counts_as_tested() {
        let idx = TestIndex {
            file_names: ["metricstest.php".to_string()].into_iter().collect(),
            contents: vec!["<?php\nclass MetricsTest {}\n".to_string()],
        };
        assert!(
            module_has_tests(Path::new("Tina4/Metrics.php"), &idx, &[]),
            "MetricsTest.php is the dedicated test for Metrics.php"
        );
    }

    #[test]
    fn a_pascalcase_match_is_anchored_not_a_substring() {
        // The negative half: `Base` must NOT be marked tested by DatabaseTest.php.
        // starts_with (not contains) is what makes the separator-less pattern safe.
        let idx = TestIndex {
            file_names: ["databasetest.php".to_string()].into_iter().collect(),
            contents: vec!["<?php\nclass DatabaseTest {}\n".to_string()],
        };
        assert!(
            !module_has_tests(Path::new("Tina4/Base.php"), &idx, &[]),
            "DatabaseTest.php tests Database, not Base"
        );
    }

}
