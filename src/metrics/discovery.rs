// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File-tree walk, glob matching and source/exclusion filtering. Split out of the metrics engine (ADR-0002); behaviour unchanged.

use super::*;

// ── File discovery ─────────────────────────────────────────────────────────────

pub(crate) const IGNORED_DIRS: &[&str] = &[
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
pub(crate) fn is_generated_asset(path: &Path) -> bool {
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
pub(crate) fn is_default_non_production_source(path: &Path) -> bool {
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
pub(crate) fn path_matches_glob(path: &str, pattern: &str) -> bool {
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

pub(crate) fn excluded_by_user(path: &Path, exclusions: &[String]) -> bool {
    let display = path.to_string_lossy();
    exclusions.iter().any(|pattern| path_matches_glob(&display, pattern))
}

/// Minified-by-content catch-all for a bundle that is not named like one.
/// A hand-written source file does not average hundreds of characters per line.
pub(crate) fn looks_minified(source: &str) -> bool {
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
pub(crate) fn is_generated_docs_dir(dir: &Path) -> bool {
    // Doxygen always emits doxygen.css alongside its HTML/JS. Sphinx always
    // emits .buildinfo at its output root.
    ["doxygen.css", "doxygen.svg", ".buildinfo"]
        .iter()
        .any(|marker| dir.join(marker).is_file())
}

pub(crate) fn walk_dir(
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
pub(crate) fn resolve_targets(
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

pub(crate) fn rel_display(file: &Path, root: &str) -> String {
    let root_path = Path::new(root);
    let rel = if root_path.is_file() {
        file.file_name().map(PathBuf::from).unwrap_or_else(|| file.to_path_buf())
    } else {
        file.strip_prefix(root_path).unwrap_or(file).to_path_buf()
    };
    rel.to_string_lossy().replace('\\', "/")
}
