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

    fn tree_sitter_language(self) -> tree_sitter::Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Lang::Ts => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
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

/// Rust's leaf kinds carry names no other grammar in the set uses, EXCEPT
/// `primitive_type`, which tree-sitter-php also emits (for `int`/`string` type
/// hints). That collision is why this list is gated on the language instead of
/// being merged into the shared one: folding it in would silently have moved
/// every PHP file's Halstead volume, and therefore its MI, with nothing in the
/// suite to catch it (the parity lock covers Python only). Gating makes
/// "the other four are untouched" true by construction rather than by luck.
fn rust_is_operand_leaf(kind: &str) -> bool {
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

fn generic_is_operand_leaf(kind: &str, lang: Lang) -> bool {
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

fn generic_halstead(
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

fn file_volume(root: Node, src: &[u8], lang: Lang) -> f64 {
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
fn parse_health(root: Node, total_lines: usize) -> f64 {
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
const MIN_PARSE_HEALTH: f64 = 0.95;

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
const MAX_AST_DEPTH: usize = 800;

/// Does any node nest deeper than `limit`?
///
/// Deliberately ITERATIVE. A recursive depth check would overflow the very stack
/// it exists to protect, on exactly the input it exists to catch.
fn depth_exceeds(root: Node, limit: usize) -> bool {
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
fn is_rust_wildcard_arm(node: Node, src: &[u8]) -> bool {
    node.child_by_field_name("pattern")
        .and_then(|p| p.utf8_text(src).ok())
        .map(|t| t.trim() == "_")
        .unwrap_or(false)
}

/// True for a node that opens a new measurement scope: a nested function (it is
/// reported as a function in its own right) or a class body (its methods are).
/// Mirrors metrics.py, which skips FunctionDef / AsyncFunctionDef / ClassDef.
fn is_scope_boundary(node: Node, lang: Lang) -> bool {
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
fn count_own_decisions(node: Node, lang: Lang, src: &[u8]) -> u32 {
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

fn is_function_node(kind: &str, lang: Lang) -> bool {
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

fn is_class_node(kind: &str, lang: Lang) -> bool {
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
fn is_type_decl_node(kind: &str, lang: Lang) -> bool {
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
fn node_name(node: Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
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

// ── Coupling (efferent) ───────────────────────────────────────────────────────

fn is_import_node(kind: &str, lang: Lang) -> bool {
    match lang {
        Lang::Python => matches!(kind, "import_statement" | "import_from_statement"),
        Lang::Php => matches!(kind, "namespace_use_declaration" | "require_once_expression"
            | "require_expression" | "include_expression" | "include_once_expression"),
        Lang::Ruby => false, // require is a plain method call — counted below
        Lang::Ts => matches!(kind, "import_statement"),
        Lang::Rust => matches!(kind, "use_declaration" | "mod_item"),
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
            Lang::Ts if kind == "call_expression" => {
                let function = node
                    .child_by_field_name("function")
                    .and_then(|function| function.utf8_text(src).ok())
                    .unwrap_or("");
                if matches!(function, "import" | "require") {
                    if let Some(specifier) = first_string_literal(node, src) {
                        specs.push(specifier);
                    }
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
            // `use crate::console::icon_ok;` -> "crate::console::icon_ok". The
            // whole path is kept as written and narrowed to a file in
            // `resolve_import`, which is the only place that knows the file set.
            Lang::Rust if kind == "use_declaration" => {
                if let Some(arg) = node.child_by_field_name("argument") {
                    if let Ok(t) = arg.utf8_text(src) {
                        // `use std::{fs, io}` -> keep the `std::` stem only; a
                        // brace group is always external here (an intra-crate
                        // group would still resolve through its stem).
                        let head = t.split('{').next().unwrap_or(t).trim().trim_end_matches("::");
                        if !head.is_empty() {
                            specs.push(head.to_string());
                        }
                    }
                }
            }
            // `mod agent;` with NO body is a declaration that agent.rs (or
            // agent/mod.rs) is part of this crate - the strongest internal edge
            // Rust has, and the one that wires main.rs to every other file in
            // this CLI. `mod x { .. }` with a body is an inline module and no
            // edge at all, so the bodyless form is what qualifies.
            Lang::Rust if kind == "mod_item" && node.child_by_field_name("body").is_none() => {
                if let Some(n) = node.child_by_field_name("name") {
                    if let Ok(t) = n.utf8_text(src) {
                        specs.push(format!("mod:{}", t.trim()));
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
        // Rust resolution, single-crate layout (what this CLI is).
        //
        // Two specifier shapes reach here. `mod:agent` is a bodyless `mod`
        // declaration and names a sibling file directly. Everything else is a
        // `use` path, where only the leading segment says whether it can be
        // internal at all: `crate` / `self` / `super` can, a bare first segment
        // (`std`, `clap`, `serde`) is an external crate and must resolve to None
        // or every third-party import would be miscounted as internal coupling.
        //
        // A `use` path mixes module segments with the ITEM it imports
        // (`crate::console::icon_ok` = module `console`, item `icon_ok`), and
        // nothing in the path says where the split is. So each prefix is offered
        // longest-first and the file set decides.
        Lang::Rust => {
            if let Some(m) = spec.strip_prefix("mod:") {
                let base = if dir.is_empty() { m.to_string() } else { format!("{dir}/{m}") };
                push(format!("{base}.rs"));
                push(format!("{base}/mod.rs"));
            } else {
                let segs: Vec<&str> = spec.split("::").filter(|s| !s.is_empty()).collect();
                let first = *segs.first()?;
                let (mut base, rest): (String, &[&str]) = match first {
                    // `crate::` is anchored at the crate root, which IS the scan
                    // root, so the base is empty rather than the current dir.
                    "crate" => (String::new(), &segs[1..]),
                    "self" => (dir.clone(), &segs[1..]),
                    "super" => {
                        let mut climbed = dir.clone();
                        let mut i = 0;
                        while segs.get(i) == Some(&"super") {
                            climbed = parent_dir(&climbed);
                            i += 1;
                        }
                        (climbed, &segs[i..])
                    }
                    _ => return None, // an external crate, excluded by design
                };
                if base == "." {
                    base = String::new();
                }
                // Longest prefix first: `console::icon_ok` tries console/icon_ok
                // before console, so a real nested module always wins over a
                // same-named item in its parent.
                for take in (1..=rest.len()).rev() {
                    let joined = rest[..take].join("/");
                    let full =
                        if base.is_empty() { joined.clone() } else { format!("{base}/{joined}") };
                    push(format!("{full}.rs"));
                    push(format!("{full}/mod.rs"));
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
const MIN_CLONE_NODES: u32 = 60;

/// Minimum SOURCE LINES a duplicate must span. Matches SonarQube's 10-line
/// default intent while staying tighter, and it is what stops a one-line
/// accessor or a short guard clause from ever being reported however many times
/// it appears.
const MIN_CLONE_LINES: usize = 6;

/// One duplicated region: where it is and how big.
///
/// `file` is an INDEX into the scanned file list rather than a String. With one
/// fragment per qualifying sub-tree, a large project produces hundreds of
/// thousands of these, and a cloned path on each is the difference between a
/// few MB and a few hundred. The hash is a u64 for the same reason - the
/// normalised shape string is never retained, only folded in.
#[derive(Clone, Debug)]
pub(crate) struct CloneFragment {
    hash: u64,
    file: u32,
    start_line: usize,
    end_line: usize,
    nodes: u32,
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
fn shape_hash_of(kind: &str, child_hashes: &[u64]) -> u64 {
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
fn is_string_leaf(kind: &str) -> bool {
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
fn mix_leaf_text(seed: u64, text: &str) -> u64 {
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
fn collect_fragments(
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
fn group_clones(mut frags: Vec<CloneFragment>, paths: &[String]) -> Vec<CloneGroup> {
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

/// Test/spec and declaration source is useful evidence, but it is not the
/// production code whose maintainability the default report promises to score.
fn is_default_non_production_source(path: &Path) -> bool {
    let normalised = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let in_test_dir = normalised
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "spec" | "__tests__"));
    in_test_dir
        || name.ends_with(".d.ts")
        || name.ends_with(".d.mts")
        || name.ends_with(".d.cts")
        || name == "conftest.py"
        || (name.starts_with("test_") && matches!(path.extension().and_then(|e| e.to_str()), Some("py") | Some("pyw")))
        || name.ends_with("_test.py")
        || name.ends_with("_test.rb")
        || name.ends_with("_spec.rb")
        || name.ends_with("test.php")
        || [".test.ts", ".test.tsx", ".test.js", ".test.jsx", ".spec.ts", ".spec.tsx", ".spec.js", ".spec.jsx"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// Small owned glob matcher for CLI exclusions. `*` stays inside one path
/// segment, `**` crosses directories, and `?` matches one non-separator byte.
/// Paths and patterns are normalised to `/`, so the rule is portable.
fn path_matches_glob(path: &str, pattern: &str) -> bool {
    fn matches_bytes(path: &[u8], pattern: &[u8], memo: &mut HashMap<(usize, usize), bool>, p: usize, g: usize) -> bool {
        if let Some(result) = memo.get(&(p, g)) {
            return *result;
        }
        let result = if g == pattern.len() {
            p == path.len()
        } else if pattern[g] == b'*' {
            let double = g + 1 < pattern.len() && pattern[g + 1] == b'*';
            if double {
                let mut next = g + 2;
                while next < pattern.len() && pattern[next] == b'*' {
                    next += 1;
                }
                if next < pattern.len() && pattern[next] == b'/' {
                    matches_bytes(path, pattern, memo, p, next + 1)
                        || (p < path.len() && matches_bytes(path, pattern, memo, p + 1, g))
                } else {
                    matches_bytes(path, pattern, memo, p, next)
                        || (p < path.len() && matches_bytes(path, pattern, memo, p + 1, g))
                }
            } else {
                matches_bytes(path, pattern, memo, p, g + 1)
                    || (p < path.len() && path[p] != b'/' && matches_bytes(path, pattern, memo, p + 1, g))
            }
        } else if pattern[g] == b'?' {
            p < path.len() && path[p] != b'/' && matches_bytes(path, pattern, memo, p + 1, g + 1)
        } else {
            p < path.len() && path[p] == pattern[g] && matches_bytes(path, pattern, memo, p + 1, g + 1)
        };
        memo.insert((p, g), result);
        result
    }

    let path = path.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    let pattern = pattern.trim_start_matches("./");
    if pattern.is_empty() {
        return false;
    }
    let candidates: Vec<&str> = if pattern.contains('/') {
        std::iter::once(path.as_str())
            .chain(path.match_indices('/').map(|(index, _)| &path[index + 1..]))
            .collect()
    } else {
        path.split('/').collect()
    };
    candidates.into_iter().any(|candidate| {
        matches_bytes(candidate.as_bytes(), pattern.as_bytes(), &mut HashMap::new(), 0, 0)
    })
}

fn excluded_by_user(path: &Path, exclusions: &[String]) -> bool {
    let display = path.to_string_lossy();
    exclusions.iter().any(|pattern| path_matches_glob(&display, pattern))
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

/// Is this directory a documentation GENERATOR's output tree?
///
/// Detected by a marker file the generator always writes, not by directory name:
/// `docs/`, `html/` and `site/` are all perfectly good places for hand-written
/// source, so banning the name would exclude real code, while the marker only
/// ever appears in generated output.
///
/// This matters because the engine is language-agnostic and therefore sees any
/// `.js` it walks past. Doxygen ships jQuery plus its own `search.js` in
/// `docs/html/`, and scanning tina4delphi produced metrics for seven Doxygen
/// files and nothing else - a report entirely about code nobody wrote. It is
/// the same class of error as measuring a file that did not parse: measuring
/// something that is not the thing.
fn is_generated_docs_dir(dir: &Path) -> bool {
    // Doxygen always emits doxygen.css alongside its HTML/JS. Sphinx always
    // emits .buildinfo at its output root.
    ["doxygen.css", "doxygen.svg", ".buildinfo"]
        .iter()
        .any(|marker| dir.join(marker).is_file())
}

fn walk_dir(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    exclusions: &[String],
    include_non_production: bool,
) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut items: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    items.sort();
    for path in items {
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || IGNORED_DIRS.contains(&name) {
                continue;
            }
            if is_generated_docs_dir(&path) {
                continue;
            }
            if excluded_by_user(&path, exclusions)
                || (!include_non_production && is_default_non_production_source(&path))
            {
                continue;
            }
            walk_dir(&path, files, exclusions, include_non_production);
        } else if Lang::from_path(&path).is_some()
            && !is_generated_asset(&path)
            && !excluded_by_user(&path, exclusions)
            && (include_non_production || !is_default_non_production_source(&path))
        {
            files.push(path);
        }
    }
}

/// Resolve the scan root(s). With `--path` honour it (file or dir). Otherwise
/// default to cwd auto-detecting `src/`, then `packages/*/src`, then `.`.
fn resolve_targets(
    path_flag: Option<&str>,
    exclusions: &[String],
    include_non_production: bool,
) -> Result<(Vec<PathBuf>, String), String> {
    let mut files = Vec::new();
    if let Some(p) = path_flag {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            if Lang::from_path(&pb).is_none() {
                return Err(format!("unsupported file type: {p}"));
            }
            let files = if excluded_by_user(&pb, exclusions)
                || (!include_non_production && is_default_non_production_source(&pb))
            {
                Vec::new()
            } else {
                vec![pb]
            };
            return Ok((files, p.to_string()));
        }
        if pb.is_dir() {
            walk_dir(&pb, &mut files, exclusions, include_non_production);
            return Ok((files, p.to_string()));
        }
        return Err(format!("Directory not found: {p}"));
    }

    let src = PathBuf::from("src");
    if src.is_dir() {
        walk_dir(&src, &mut files, exclusions, include_non_production);
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
                    walk_dir(&pkg_src, &mut files, exclusions, include_non_production);
                }
            }
        }
        if !files.is_empty() {
            return Ok((files, "packages/*/src".to_string()));
        }
    }
    walk_dir(Path::new("."), &mut files, exclusions, include_non_production);
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

#[derive(Default)]
struct TestIndex {
    file_names: HashSet<String>,
    contents: Vec<String>,
    import_subjects: HashMap<Lang, HashSet<String>>,
}

fn normalise_reference_subject(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl TestIndex {
    fn add(&mut self, file: &Path, content: String) {
        if let Some(name) = file.file_name().and_then(|name| name.to_str()) {
            self.file_names.insert(name.to_ascii_lowercase());
        }
        if let Some(lang) = Lang::from_path(file) {
            let mut parser = Parser::new();
            if parser.set_language(&lang.tree_sitter_language()).is_ok() {
                if let Some(tree) = parser.parse(&content, None) {
                    let subjects = self.import_subjects.entry(lang).or_default();
                    for specifier in extract_import_specs(tree.root_node(), lang, content.as_bytes()) {
                        let without_extension = [
                            ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".py", ".php", ".rb", ".rs",
                        ]
                        .iter()
                        .find_map(|extension| specifier.strip_suffix(extension))
                        .unwrap_or(&specifier);
                        let subject = without_extension
                            .rsplit(['/', '\\', '.', ':'])
                            .find(|part| !part.is_empty())
                            .unwrap_or("");
                        subjects.insert(normalise_reference_subject(subject));
                    }
                }
            }
        }
        self.contents.push(content);
    }
}

fn build_test_index(root: &str) -> TestIndex {
    let mut index = TestIndex::default();
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
                walk_dir(&dir, &mut tf, &[], true);
                for f in tf {
                    if let Ok(c) = fs::read_to_string(&f) {
                        index.add(&f, c);
                    }
                }
            }
        }
    }
    index
}

/// Publicly referenceable names declared by this source file.
///
/// A test may import through a package barrel and never mention the file stem.
/// Named types and top-level callables are therefore valid reference signals.
/// Nested helpers and class methods are excluded because their short names
/// collide constantly and the containing type is the honest public subject.
fn declared_reference_names(source: &str, lang: Lang) -> Vec<String> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter_language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else { return Vec::new() };
    let bytes = source.as_bytes();
    let mut names = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let top_level_callable = node.is_named()
            && is_function_node(node.kind(), lang)
            && {
                let mut parent = node.parent();
                let mut nested = false;
                while let Some(ancestor) = parent {
                    if (ancestor.is_named() && is_function_node(ancestor.kind(), lang))
                        || (is_class_node(ancestor.kind(), lang)
                            && !(lang == Lang::Ruby && ancestor.kind() == "module"))
                    {
                        nested = true;
                        break;
                    }
                    parent = ancestor.parent();
                }
                !nested
            };
        if is_type_decl_node(node.kind(), lang) || top_level_callable {
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
    // Stage 2: a parsed import/require/use/dynamic-import specifier that names
    // this module. Tests are parsed once while building the index, not once per
    // production file; this keeps the scan linear in the corpus size.
    let source_lang = Lang::from_path(file);
    if source_lang
        .and_then(|lang| idx.import_subjects.get(&lang))
        .is_some_and(|subjects| subjects.contains(&normalise_reference_subject(stem)))
    {
        return true;
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

/// Rust keeps its unit tests INSIDE the file they test, behind `#[cfg(test)]`,
/// rather than in a sibling `tests/` tree. Every stage of `module_has_tests`
/// looks for an external test file, so without this the convention reads as "no
/// test anywhere" and every single .rs file raises a false `untested` offender —
/// which is exactly the noise that makes an offender list get ignored.
///
/// Matched on the source text, not the AST: `#[cfg(test)]` is an attribute whose
/// node shape varies across grammar versions, and the string is unambiguous.
fn rust_has_inline_tests(source: &str) -> bool {
    source.contains("#[cfg(test)]")
}

/// A file the engine REFUSED to measure because too little of it parsed.
///
/// Deliberately a first-class part of the report rather than an omission. A
/// refused file is excluded from `file_metrics` so it cannot skew an average
/// with numbers derived from rubble, and listed here so it is never merely
/// ABSENT - "the engine could not read this" and "this file is fine" must not
/// look the same to anyone reading the output.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct RefusedFile {
    pub path: String,
    /// Fraction of lines that parsed cleanly, rounded to 3 dp.
    pub parse_health: f64,
    pub lines: usize,
    pub reason: String,
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
                // ASCII only. Every other `detail` here already is, and this one
                // em dash made the payload non-ASCII: a consumer whose runtime
                // tags subprocess output with a minimal locale (LANG=C) then
                // raised on the first string operation. Also the house style.
                detail: format!("{} - cyclomatic complexity {}", fn_info.name, cc),
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
            });
        }
    }

    sort_offenders(&mut items);
    items
}

/// Sort by (severity rank, score) descending, stable.
fn sort_offenders(items: &mut [Offender]) {
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
fn refusal_offenders(refused: &[RefusedFile]) -> Vec<Offender> {
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
fn clone_offenders(groups: &[CloneGroup]) -> Vec<Offender> {
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
            }
        })
        .collect()
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
    /// Maximal duplicated blocks found across the whole scan, and the lines that
    /// would disappear if each group were unified to a single definition. Whole-
    /// scan facts, so they live on the summary rather than on any one file.
    duplicate_blocks: usize,
    duplicate_lines: usize,
    /// Files the parse-health guard REFUSED. Reported so a hole in the coverage
    /// is never invisible - the whole point of the guard.
    files_refused: usize,
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
    /// Maximal duplicated AST shapes, biggest payoff first.
    duplication: Vec<CloneGroup>,
    /// Files excluded from every number above because too little of them
    /// parsed. Present even when empty so a consumer can rely on the key.
    unparsed: Vec<RefusedFile>,
    /// What changed since the previous recorded run of the SAME scan root.
    /// `null` on the first run (nothing to compare) or when `--no-history` is
    /// set. See the run-history section below.
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<RunDelta>,
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
    clones: Vec<CloneGroup>,
    refused: Vec<RefusedFile>,
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
        duplicate_blocks: report.clones.len(),
        duplicate_lines: report.clones.iter().map(|c| c.lines * (c.copies - 1)).sum(),
        files_refused: report.refused.len(),
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

// ── Run history: regression / improvement tracking ──────────────────────────
// Every scan records a compact snapshot to `.tina4-metrics.json` in the scan
// root, so the NEXT run of the same scope can say what improved and what got
// worse. Zero new dependencies: the record is serde_json and timestamps are
// UNIX seconds from SystemTime. `--no-history` reads and writes nothing.
//
// The file is data, not source (a `.json`), so the engine never scans it and it
// never pollutes its own numbers. Commit it to gate regressions in CI, or add it
// to .gitignore for a purely local baseline - both work.

const HISTORY_FILE: &str = ".tina4-metrics.json";
const HISTORY_SCHEMA: u32 = 1;
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

#[derive(Serialize, Deserialize, Clone, Default)]
struct HistoryFile {
    offenders: usize,
    /// Highest cyclomatic complexity of any function in the file.
    worst_cc: u32,
    maintainability: f64,
    loc: usize,
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
}

/// One file's movement between two runs.
#[derive(Serialize)]
struct FileDelta {
    path: String,
    offenders_before: i64,
    offenders_after: i64,
    worst_cc_before: i64,
    worst_cc_after: i64,
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

/// Per-file offender count + worst function complexity for this run, the two
/// signals a next run diffs to say a file improved or regressed.
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
    map
}

fn compute_delta(
    prev: &HistoryRecord,
    after: &MetricsSnapshot,
    cur_files: &BTreeMap<String, HistoryFile>,
) -> RunDelta {
    let mut improved: Vec<FileDelta> = Vec::new();
    let mut regressed: Vec<FileDelta> = Vec::new();

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
                status: "improved".to_string(),
            });
        } else if oa > ob || (cc_moves && wa > wb) {
            regressed.push(FileDelta {
                path: path.clone(),
                offenders_before: ob,
                offenders_after: oa,
                worst_cc_before: wb,
                worst_cc_after: wa,
                status: "regressed".to_string(),
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
                status: "resolved".to_string(),
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

    RunDelta {
        since_secs: now_secs().saturating_sub(prev.last.at),
        before: prev.last.clone(),
        after: after.clone(),
        improved_files: improved,
        regressed_files: regressed,
    }
}

fn save_history(
    scan_root: &str,
    prev: Option<HistoryRecord>,
    last: MetricsSnapshot,
    files: BTreeMap<String, HistoryFile>,
) {
    let mut trend: Vec<MetricsSnapshot> = Vec::new();
    if let Some(p) = prev {
        trend = p.trend;
        trend.push(p.last); // the previous run's summary joins the trend line
        let overflow = trend.len().saturating_sub(TREND_CAP);
        if overflow > 0 {
            trend.drain(0..overflow);
        }
    }
    let rec = HistoryRecord {
        schema: HISTORY_SCHEMA,
        scan_root: scan_root.to_string(),
        last,
        files,
        trend,
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

/// `tina4 metrics` — native, language-agnostic. Returns the process exit code.
pub fn run(
    path: Option<String>,
    top: Option<usize>,
    json: bool,
    fail_on: Option<String>,
    exclusions: Vec<String>,
    include_non_production: bool,
    no_history: bool,
) -> i32 {
    if let Some(f) = &fail_on {
        if f != "warn" && f != "error" {
            eprintln!("  invalid --fail-on '{f}' (use warn or error)");
            return 2;
        }
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
            duplication: report.clones.clone(),
            unparsed: report.refused.clone(),
            delta,
        };
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()));
        if !no_history {
            save_history(&scan_root, history_prev, cur_snapshot, cur_files);
        }
        return exit_code;
    }

    print_human(&summary, &shown);
    match &delta {
        Some(d) => print_delta_human(d, &scan_root),
        None if !no_history => println!(
            "\n  Baseline saved to {} - rerun to see what changed.",
            history_path(&scan_root).display()
        ),
        None => {}
    }
    if !no_history {
        save_history(&scan_root, history_prev, cur_snapshot, cur_files);
    }
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
        HistoryFile { offenders: off, worst_cc: cc, maintainability: 50.0, loc: 100 }
    }
    fn record(files: BTreeMap<String, HistoryFile>, last: MetricsSnapshot) -> HistoryRecord {
        HistoryRecord { schema: HISTORY_SCHEMA, scan_root: ".".to_string(), last, files, trend: vec![] }
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
    fn history_ignores_cc_wobble_on_a_clean_file() {
        // A file with zero offenders whose worst complexity rises is summary noise,
        // NOT a per-file regression - it must not appear in either list.
        let mut prev_files = BTreeMap::new();
        prev_files.insert("a.py".to_string(), hfile(0, 3));
        let prev = record(prev_files, snap(0, 0, 80.0, 1.0));
        let mut cur = BTreeMap::new();
        cur.insert("a.py".to_string(), hfile(0, 9));
        let d = compute_delta(&prev, &snap(0, 0, 60.0, 5.0), &cur);
        assert!(
            d.improved_files.is_empty() && d.regressed_files.is_empty(),
            "cc wobble on a clean file is not a per-file change"
        );
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
        let _ = fs::remove_dir_all(&dir);
    }
}
