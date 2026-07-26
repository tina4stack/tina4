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

use std::collections::HashSet;
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

#[derive(Clone, Debug)]
pub(crate) struct FunctionInfo {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub complexity: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct FileMetrics {
    pub path: String,
    pub loc: usize,
    pub complexity: u32,      // file_complexity: sum of every function's CC
    pub avg_complexity: f64,  // rounded to 2 dp
    pub functions: usize,
    pub maintainability: f64, // rounded to 1 dp, clamped [0, 100]
    pub coupling_efferent: usize,
    pub has_tests: bool,
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

fn count_decisions(node: Node, lang: Lang, src: &[u8]) -> u32 {
    let mut total = is_decision(node, lang, src);
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            total += count_decisions(c.node(), lang, src);
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
        out.push((node, count_decisions(node, lang, src) + 1));
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

fn count_imports(root: Node, lang: Lang, src: &[u8]) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_import_node(node.kind(), lang) {
            count += 1;
        } else if lang == Lang::Ruby && node.kind() == "call" {
            if let Some(m) = node.child_by_field_name("method").and_then(|n| n.utf8_text(src).ok()) {
                if matches!(m, "require" | "require_relative" | "load" | "autoload") {
                    count += 1;
                }
            }
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            stack.push(child);
        }
    }
    count
}

// ── Analyze one source string ─────────────────────────────────────────────────

pub(crate) fn analyze_source(
    lang: Lang,
    source: &str,
    rel_path: &str,
    has_tests: bool,
) -> Option<(FileMetrics, Vec<FunctionInfo>)> {
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
        functions.push(FunctionInfo {
            name: function_display_name(*node, lang, src),
            file: rel_path.to_string(),
            line: node.start_position().row + 1,
            complexity: *cc,
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
    let coupling_efferent = count_imports(root, lang, src);

    let fm = FileMetrics {
        path: rel_path.to_string(),
        loc,
        complexity: file_complexity,
        avg_complexity: round_dp(avg_cc, 2),
        functions: num_functions,
        maintainability: mi,
        coupling_efferent,
        has_tests,
    };
    Some((fm, functions))
}

// ── File discovery ─────────────────────────────────────────────────────────────

const IGNORED_DIRS: &[&str] = &[
    "node_modules", "vendor", ".git", "target", "dist", "build", "__pycache__",
    ".venv", "venv", "coverage", ".next", "out", ".tina4-docs", ".idea", ".pytest_cache",
    ".mypy_cache", ".ruff_cache", "site-packages",
];

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
        } else if Lang::from_path(&path).is_some() {
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

fn module_has_tests(file: &Path, idx: &TestIndex) -> bool {
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
    for pat in [
        format!("test_{stem_l}."),
        format!("test_{stem_l}s."),
        format!("{stem_l}_test."),
        format!("{stem_l}_spec."),
        format!("{stem_l}.test."),
        format!("{stem_l}.spec."),
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
}

pub(crate) struct Report {
    files: Vec<FileMetrics>,
    functions: Vec<FunctionInfo>,
    offenders: Vec<Offender>,
    scan_root: String,
}

pub(crate) fn analyze_targets(files: &[PathBuf], scan_root: &str) -> Report {
    let test_index = build_test_index(scan_root);
    let mut file_metrics: Vec<FileMetrics> = Vec::new();
    let mut all_functions: Vec<FunctionInfo> = Vec::new();

    for path in files {
        let Some(lang) = Lang::from_path(path) else { continue };
        let Ok(source) = fs::read_to_string(path) else { continue };
        let rel = rel_display(path, scan_root);
        let has_tests = module_has_tests(path, &test_index);
        if let Some((fm, funcs)) = analyze_source(lang, &source, &rel, has_tests) {
            file_metrics.push(fm);
            all_functions.extend(funcs);
        }
    }

    let offenders = build_offenders(&file_metrics, &all_functions);
    Report { files: file_metrics, functions: all_functions, offenders, scan_root: scan_root.to_string() }
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
        let payload = JsonPayload { summary, offenders: shown };
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
        analyze_source(Lang::Python, src, "t.py", false).unwrap()
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

    // ---- Offender rules (mirror metrics.py thresholds) -----------------------

    fn file_with(mi: f64, loc: usize, funcs: usize, has_tests: bool) -> FileMetrics {
        FileMetrics {
            path: "x.py".into(), loc, complexity: 0, avg_complexity: 0.0,
            functions: funcs, maintainability: mi, coupling_efferent: 0, has_tests,
        }
    }
    fn func_with(cc: u32) -> FunctionInfo {
        FunctionInfo { name: "f".into(), file: "x.py".into(), line: 1, complexity: cc }
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
        let (fm, fns) = analyze_source(Lang::Php, php, "t.php", false).unwrap();
        assert_eq!(fm.functions, 1);
        assert!(fns[0].complexity >= 3); // 1 + if + &&
        assert!(fm.maintainability > 0.0 && fm.maintainability <= 100.0);

        let rb = "def f(a)\n  return 1 if a && a > 0\n  0\nend\n";
        let (fm, _f) = analyze_source(Lang::Ruby, rb, "t.rb", false).unwrap();
        assert_eq!(fm.functions, 1);

        let ts = "export const f = (a: number) => { if (a && a > 0) { return 1; } return 0; };\n";
        let (fm, fns) = analyze_source(Lang::Ts, ts, "t.ts", false).unwrap();
        assert_eq!(fm.functions, 1, "top-level arrow function is counted");
        assert!(fns[0].complexity >= 3);
    }

    #[test]
    fn typescript_import_coupling_counts() {
        let ts = "import { a } from './a';\nimport b from './b';\nconst x = () => a + b;\n";
        let (fm, _f) = analyze_source(Lang::Ts, ts, "t.ts", false).unwrap();
        assert_eq!(fm.coupling_efferent, 2);
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
}
