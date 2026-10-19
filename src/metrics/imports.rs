// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-language import extraction and internal coupling resolution. Split out of the metrics engine (ADR-0002); behaviour
//! unchanged. `use super::*` shares Lang and the tree-sitter types.

use super::*;

// ── Coupling (efferent) ───────────────────────────────────────────────────────

pub(crate) fn is_import_node(kind: &str, lang: Lang) -> bool {
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
pub(crate) fn unquote(raw: &str) -> String {
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
pub(crate) fn first_string_literal(node: Node, src: &[u8]) -> Option<String> {
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
pub(crate) fn extract_import_specs(root: Node, lang: Lang, src: &[u8]) -> Vec<String> {
    let mut specs: Vec<String> = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        match lang {
            Lang::Python => py_import_specs(node, kind, src, &mut specs),
            Lang::Ts => ts_import_specs(node, kind, src, &mut specs),
            Lang::Php => php_import_specs(node, kind, src, &mut specs),
            Lang::Rust => rust_import_specs(node, kind, src, &mut specs),
            Lang::Ruby => ruby_import_specs(node, kind, src, &mut specs),
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

/// Python import specifiers for one AST node (split out of extract_import_specs).
pub(crate) fn py_import_specs(node: Node, kind: &str, src: &[u8], specs: &mut Vec<String>) {
    if kind == "import_from_statement" {
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
    if kind == "import_statement" {
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
}

/// TypeScript/JS import specifiers for one AST node.
pub(crate) fn ts_import_specs(node: Node, kind: &str, src: &[u8], specs: &mut Vec<String>) {
    if kind == "import_statement" {
        if let Some(s) = node.child_by_field_name("source") {
            if let Ok(t) = s.utf8_text(src) {
                specs.push(unquote(t));
            }
        } else if let Some(s) = first_string_literal(node, src) {
            specs.push(s);
        }
    }
    if kind == "call_expression" {
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
}

/// PHP import specifiers for one AST node (use-clauses, require/include, and the
/// dominant inline fully-qualified reference idiom).
pub(crate) fn php_import_specs(node: Node, kind: &str, src: &[u8], specs: &mut Vec<String>) {
    // `use Tina4\Frond;` and `use Tina4\{A, B};` nest the path one level
    // down, inside a namespace_use_clause / -group-clause - matching only
    // the outer declaration's DIRECT children found nothing at all, which
    // is why PHP produced zero edges across 138 files.
    if matches!(kind, "namespace_use_clause" | "namespace_use_group_clause") {
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
    if matches!(kind, "require_once_expression" | "require_expression" | "include_expression" | "include_once_expression") {
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
    if kind.contains("qualified_name") {
        if let Ok(t) = node.utf8_text(src) {
            let t = t.trim();
            if t.contains('\\') && t.len() > 1 {
                specs.push(t.to_string());
            }
        }
    }
}

/// Rust import specifiers for one AST node (use-declarations and bodyless mod items).
pub(crate) fn rust_import_specs(node: Node, kind: &str, src: &[u8], specs: &mut Vec<String>) {
    // `use crate::console::icon_ok;` -> "crate::console::icon_ok". The
    // whole path is kept as written and narrowed to a file in
    // `resolve_import`, which is the only place that knows the file set.
    if kind == "use_declaration" {
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
    if kind == "mod_item" && node.child_by_field_name("body").is_none() {
        if let Some(n) = node.child_by_field_name("name") {
            if let Ok(t) = n.utf8_text(src) {
                specs.push(format!("mod:{}", t.trim()));
            }
        }
    }
}

/// Ruby import specifiers for one AST node (require/require_relative/load/autoload).
pub(crate) fn ruby_import_specs(node: Node, kind: &str, src: &[u8], specs: &mut Vec<String>) {
    if kind == "call" {
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
}

/// Normalise a candidate path: drop `./`, collapse `a/../b`, unify separators.
pub(crate) fn normalise_path(raw: &str) -> String {
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
pub(crate) fn parent_dir(rel: &str) -> String {
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
pub(crate) fn resolve_import(
    spec: &str,
    from_rel: &str,
    lang: Lang,
    paths: &HashSet<String>,
    root_pkg: Option<&str>,
) -> Option<String> {
    let dir = parent_dir(from_rel);
    let candidates = match lang {
        Lang::Python => py_resolve_candidates(spec, &dir, root_pkg),
        Lang::Ts => ts_resolve_candidates(spec, &dir),
        Lang::Ruby => ruby_resolve_candidates(spec, &dir, root_pkg),
        Lang::Rust => rust_resolve_candidates(spec, &dir),
        Lang::Php => php_resolve_candidates(spec, &dir, root_pkg),
    };

    candidates
        .into_iter()
        .find(|c| !c.is_empty() && c != from_rel && paths.contains(c))
}

/// Python: relative dots climb package levels; absolute names map onto the
/// scan tree, with the scan root's basename as an implicit package prefix.
pub(crate) fn py_resolve_candidates(spec: &str, dir: &str, root_pkg: Option<&str>) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));
    // Leading dots = relative import; each dot climbs one package level.
    let dots = spec.chars().take_while(|c| *c == '.').count();
    let tail = spec.trim_start_matches('.').replace('.', "/");
    if dots > 0 {
        let mut base = dir.to_string();
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
    candidates
}

/// TypeScript / JS: only a relative specifier can be internal; probe the usual
/// extensions and index files, and treat a ".js" import as the ".ts" source.
pub(crate) fn ts_resolve_candidates(spec: &str, dir: &str) -> Vec<String> {
    // Only a relative specifier can be internal; bare ones are packages.
    if !(spec.starts_with('.') || spec.starts_with('/')) {
        return Vec::new();
    }
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));
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
    candidates
}

/// Ruby: require_relative is dir-relative; require is load-path-relative.
pub(crate) fn ruby_resolve_candidates(spec: &str, dir: &str, root_pkg: Option<&str>) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));
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
    candidates
}

/// Rust resolution, single-crate layout (what this CLI is).
///
/// Two specifier shapes reach here. `mod:agent` is a bodyless `mod`
/// declaration and names a sibling file directly. Everything else is a
/// `use` path, where only the leading segment says whether it can be
/// internal at all: `crate` / `self` / `super` can, a bare first segment
/// (`std`, `clap`, `serde`) is an external crate and must resolve to None
/// or every third-party import would be miscounted as internal coupling.
///
/// A `use` path mixes module segments with the ITEM it imports
/// (`crate::console::icon_ok` = module `console`, item `icon_ok`), and
/// nothing in the path says where the split is. So each prefix is offered
/// longest-first and the file set decides.
pub(crate) fn rust_resolve_candidates(spec: &str, dir: &str) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));
    if let Some(m) = spec.strip_prefix("mod:") {
        let base = if dir.is_empty() { m.to_string() } else { format!("{dir}/{m}") };
        push(format!("{base}.rs"));
        push(format!("{base}/mod.rs"));
    } else {
        let segs: Vec<&str> = spec.split("::").filter(|s| !s.is_empty()).collect();
        let first = match segs.first() {
            Some(s) => *s,
            None => return Vec::new(),
        };
        let (mut base, rest): (String, &[&str]) = match first {
            // `crate::` is anchored at the crate root, which IS the scan
            // root, so the base is empty rather than the current dir.
            "crate" => (String::new(), &segs[1..]),
            "self" => (dir.to_string(), &segs[1..]),
            "super" => {
                let mut climbed = dir.to_string();
                let mut i = 0;
                while segs.get(i) == Some(&"super") {
                    climbed = parent_dir(&climbed);
                    i += 1;
                }
                (climbed, &segs[i..])
            }
            _ => return Vec::new(), // an external crate, excluded by design
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
    candidates
}

/// PHP: a `use` path is a PSR-4 namespace mapped onto directories; also
/// handles require/include literals, possibly dir-relative.
pub(crate) fn php_resolve_candidates(spec: &str, dir: &str, root_pkg: Option<&str>) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |c: String| candidates.push(normalise_path(&c));
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
    candidates
}
