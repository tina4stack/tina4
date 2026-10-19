// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::Path;

/// Information about a detected Tina4 project.
#[derive(Clone)]
pub struct ProjectInfo {
    pub language: String,
    /// The project's declared version from its manifest (pyproject `version`,
    /// or the top-level `version` in composer.json / package.json), when present.
    /// Shown by `tina4 serve` and `tina4 doctor`.
    pub version: Option<String>,
}

impl ProjectInfo {
    /// Returns the language-specific CLI binary name.
    pub fn cli_name(&self) -> &str {
        match self.language.as_str() {
            "python" => "tina4python",
            "php" => "tina4php",
            "ruby" => "tina4ruby",
            "nodejs" => "tina4nodejs",
            "tina4js" => "vite",
            _ => "tina4",
        }
    }

    /// Returns the default port for this framework.
    pub fn default_port(&self) -> u16 {
        match self.language.as_str() {
            "python" => 7146,
            "php" => 7145,
            "ruby" => 7147,
            "nodejs" => 7148,
            "tina4js" => 5173,
            _ => 7145,
        }
    }
}

/// Detect the Tina4 project language from files in the current directory.
///
/// Checks for:
///   - pyproject.toml or requirements.txt with tina4 → Python
///   - composer.json with tina4 → PHP
///   - Gemfile or tina4ruby.gemspec → Ruby
///   - package.json with @tina4 → Node.js
pub fn detect_language() -> Option<ProjectInfo> {
    detect_language_in(Path::new("."))
}

/// Path-aware core of [`detect_language`], which only ever inspects the current
/// directory. The evaluation order is load-bearing: a directory can carry
/// several markers at once (app.rb beside index.php, composer.json beside a
/// Gemfile), and the first match wins. These helpers run in the exact order the
/// one long function used to, so the outcome is unchanged - the split only keeps
/// each language's branching small enough to read.
pub fn detect_language_in(dir: &Path) -> Option<ProjectInfo> {
    // One detector per language, run in the original order. The first match wins.
    let detectors: [fn(&Path) -> Option<ProjectInfo>; 5] = [
        detect_python_project,
        detect_php_manifest,
        detect_ruby_manifest,
        detect_node_project,
        detect_trailing_entrypoints,
    ];
    detectors.into_iter().find_map(|detect| detect(dir))
}

/// Python: pyproject.toml or requirements.txt that names tina4, or a bare app.py.
fn detect_python_project(dir: &Path) -> Option<ProjectInfo> {
    let pyproject = dir.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            if content.contains("tina4") {
                return Some(ProjectInfo {
                    language: "python".into(),
                    version: extract_version_toml(&content),
                });
            }
        }
    }
    let requirements = dir.join("requirements.txt");
    if requirements.exists() {
        if let Ok(content) = std::fs::read_to_string(&requirements) {
            if content.to_lowercase().contains("tina4") {
                return Some(ProjectInfo { language: "python".into(), version: None });
            }
        }
    }
    if dir.join("app.py").exists() {
        return Some(ProjectInfo { language: "python".into(), version: None });
    }
    None
}

/// PHP manifest: composer.json that names tina4. (A bare index.php is a trailing fallback.)
fn detect_php_manifest(dir: &Path) -> Option<ProjectInfo> {
    let composer = dir.join("composer.json");
    if composer.exists() {
        if let Ok(content) = std::fs::read_to_string(&composer) {
            if content.contains("tina4") {
                return Some(ProjectInfo {
                    language: "php".into(),
                    version: extract_version_json(&content, "version"),
                });
            }
        }
    }
    None
}

/// Ruby manifest: a Gemfile that names tina4, or tina4ruby.gemspec. (A bare app.rb is a trailing fallback.)
fn detect_ruby_manifest(dir: &Path) -> Option<ProjectInfo> {
    let gemfile = dir.join("Gemfile");
    if gemfile.exists() {
        if let Ok(content) = std::fs::read_to_string(&gemfile) {
            if content.contains("tina4") {
                return Some(ProjectInfo { language: "ruby".into(), version: None });
            }
        }
    }
    if dir.join("tina4ruby.gemspec").exists() {
        return Some(ProjectInfo { language: "ruby".into(), version: None });
    }
    None
}

/// Node.js / tina4js: package.json (tina4js frontend vs tina4-nodejs backend), or a bare app.ts.
fn detect_node_project(dir: &Path) -> Option<ProjectInfo> {
    let package = dir.join("package.json");
    if package.exists() {
        if let Ok(content) = std::fs::read_to_string(&package) {
            // tina4js (frontend) — has "tina4js" dep but NOT "tina4-nodejs"
            if content.contains("\"tina4js\"") && !content.contains("tina4-nodejs") && !content.contains("tina4nodejs") {
                return Some(ProjectInfo {
                    language: "tina4js".into(),
                    version: extract_version_json(&content, "version"),
                });
            }
            if content.contains("@tina4") || content.contains("tina4-nodejs") || content.contains("tina4nodejs") {
                return Some(ProjectInfo {
                    language: "nodejs".into(),
                    version: extract_version_json(&content, "version"),
                });
            }
        }
    }
    if dir.join("app.ts").exists() {
        return Some(ProjectInfo { language: "nodejs".into(), version: None });
    }
    None
}

/// Bare entry-point fallbacks, kept last and in their original order: app.rb then index.php.
fn detect_trailing_entrypoints(dir: &Path) -> Option<ProjectInfo> {
    if dir.join("app.rb").exists() {
        return Some(ProjectInfo { language: "ruby".into(), version: None });
    }
    if dir.join("index.php").exists() {
        return Some(ProjectInfo { language: "php".into(), version: None });
    }
    None
}

/// Does `dir` look like a Tina4 project? Path-aware sibling of
/// `detect_language` (which only inspects the current directory). Used by
/// `tina4 serve` to find a project to launch in the configured projects folder.
/// Checks the same entry-point markers detect_language relies on.
pub fn dir_is_tina4_project(dir: &Path) -> bool {
    // Language entry points — any one is enough.
    for entry in ["app.py", "app.ts", "app.js", "app.rb", "index.php"] {
        if dir.join(entry).exists() {
            return true;
        }
    }
    // Python via pyproject.toml that mentions tina4.
    let pyproject = dir.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            if content.contains("tina4") {
                return true;
            }
        }
    }
    false
}

fn extract_version_toml(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version") && trimmed.contains('=') {
            let val = trimmed.split('=').nth(1)?.trim();
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

fn extract_version_json(content: &str, key: &str) -> Option<String> {
    // Simple JSON value extraction without a full parser
    let pattern = format!("\"{}\"", key);
    let pos = content.find(&pattern)?;
    let rest = &content[pos + pattern.len()..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    if let Some(stripped) = after_colon.strip_prefix('"') {
        let end = stripped.find('"')?;
        return Some(stripped[..end].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fresh, unique temp directory per call - path-aware detection needs no
    /// chdir, so these run in parallel without clobbering each other's CWD.
    fn fixture(files: &[(&str, &str)]) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("tina4_detect_{}_{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        dir
    }

    fn lang(dir: &std::path::Path) -> Option<String> {
        detect_language_in(dir).map(|p| p.language)
    }

    #[test]
    fn detects_each_language_from_its_manifest() {
        let py = fixture(&[("pyproject.toml", "[project]\nname='x'\ndependencies=['tina4']\nversion = \"1.2.3\"\n")]);
        let info = detect_language_in(&py).unwrap();
        assert_eq!(info.language, "python");
        assert_eq!(info.version.as_deref(), Some("1.2.3"));

        assert_eq!(lang(&fixture(&[("requirements.txt", "Tina4\n")])).as_deref(), Some("python"));
        assert_eq!(lang(&fixture(&[("app.py", "")])).as_deref(), Some("python"));
        assert_eq!(lang(&fixture(&[("composer.json", "{\"require\":{\"tina4/tina4\":\"*\"}}")])).as_deref(), Some("php"));
        assert_eq!(lang(&fixture(&[("Gemfile", "gem 'tina4'\n")])).as_deref(), Some("ruby"));
        assert_eq!(lang(&fixture(&[("tina4ruby.gemspec", "")])).as_deref(), Some("ruby"));
        assert_eq!(lang(&fixture(&[("package.json", "{\"dependencies\":{\"tina4-nodejs\":\"*\"}}")])).as_deref(), Some("nodejs"));
        assert_eq!(lang(&fixture(&[("package.json", "{\"dependencies\":{\"tina4js\":\"*\"}}")])).as_deref(), Some("tina4js"));
    }

    #[test]
    fn unmarked_or_empty_directory_is_none() {
        assert_eq!(lang(&fixture(&[])), None);
        // A manifest that does not name tina4 is not a Tina4 project.
        assert_eq!(lang(&fixture(&[("composer.json", "{\"require\":{\"other/pkg\":\"*\"}}")])), None);
    }

    #[test]
    fn evaluation_order_is_preserved() {
        // These pin the load-bearing order; reordering the or_else chain or the
        // trailing checks breaks them (mutation gate).
        // Trailing: app.rb is checked before index.php.
        assert_eq!(lang(&fixture(&[("app.rb", ""), ("index.php", "")])).as_deref(), Some("ruby"));
        // A real PHP manifest beats a bare Node entry point (php runs before node).
        assert_eq!(
            lang(&fixture(&[("composer.json", "{\"require\":{\"tina4/tina4\":\"*\"}}"), ("app.ts", "")])).as_deref(),
            Some("php"),
        );
        // Python manifest beats a PHP manifest (python runs first).
        assert_eq!(
            lang(&fixture(&[
                ("pyproject.toml", "dependencies=['tina4']"),
                ("composer.json", "{\"require\":{\"tina4/tina4\":\"*\"}}"),
            ])).as_deref(),
            Some("python"),
        );
    }
}
