// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pragmatic test-reference detection for the untested-module offender. Split out of the metrics engine (ADR-0002); behaviour unchanged.

use super::*;

// ── Test detection (pragmatic; drives only the `info untested` offender) ────────
// A best-effort, language-agnostic reimplementation — NOT a byte-exact port of
// metrics.py's Python-dotted-path heuristic. It never affects `--fail-on`
// (untested is `info`). Signals: a dedicated test file named for the module, or a
// test file that mentions the module stem on an import/require/use line.

#[derive(Default)]
pub(crate) struct TestIndex {
    pub(crate) file_names: HashSet<String>,
    pub(crate) contents: Vec<String>,
    pub(crate) import_subjects: HashMap<Lang, HashSet<String>>,
}

pub(crate) fn normalise_reference_subject(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl TestIndex {
    pub(crate) fn add(&mut self, file: &Path, content: String) {
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

pub(crate) fn build_test_index(root: &str) -> TestIndex {
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
pub(crate) fn declared_reference_names(source: &str, lang: Lang) -> Vec<String> {
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
pub(crate) fn mentions_symbol(haystack: &str, needle: &str) -> bool {
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

pub(crate) fn module_has_tests(file: &Path, idx: &TestIndex, declared_types: &[String]) -> bool {
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
pub(crate) fn rust_has_inline_tests(source: &str) -> bool {
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
