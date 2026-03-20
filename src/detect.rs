use std::path::Path;

/// Information about a detected Tina4 project.
pub struct ProjectInfo {
    pub language: String,
    #[allow(dead_code)]
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
            _ => "tina4",
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
    // Python: pyproject.toml or requirements.txt
    if Path::new("pyproject.toml").exists() {
        if let Ok(content) = std::fs::read_to_string("pyproject.toml") {
            if content.contains("tina4") {
                return Some(ProjectInfo {
                    language: "python".into(),
                    version: extract_version_toml(&content),
                });
            }
        }
    }
    if Path::new("requirements.txt").exists() {
        if let Ok(content) = std::fs::read_to_string("requirements.txt") {
            if content.to_lowercase().contains("tina4") {
                return Some(ProjectInfo {
                    language: "python".into(),
                    version: None,
                });
            }
        }
    }
    // Also detect app.py as a Python project
    if Path::new("app.py").exists() {
        return Some(ProjectInfo {
            language: "python".into(),
            version: None,
        });
    }

    // PHP: composer.json
    if Path::new("composer.json").exists() {
        if let Ok(content) = std::fs::read_to_string("composer.json") {
            if content.contains("tina4") {
                return Some(ProjectInfo {
                    language: "php".into(),
                    version: extract_version_json(&content, "version"),
                });
            }
        }
    }

    // Ruby: Gemfile or gemspec
    if Path::new("Gemfile").exists() {
        if let Ok(content) = std::fs::read_to_string("Gemfile") {
            if content.contains("tina4") {
                return Some(ProjectInfo {
                    language: "ruby".into(),
                    version: None,
                });
            }
        }
    }
    if Path::new("tina4ruby.gemspec").exists() {
        return Some(ProjectInfo {
            language: "ruby".into(),
            version: None,
        });
    }

    // Node.js: package.json
    if Path::new("package.json").exists() {
        if let Ok(content) = std::fs::read_to_string("package.json") {
            if content.contains("@tina4") || content.contains("tina4nodejs") {
                return Some(ProjectInfo {
                    language: "nodejs".into(),
                    version: extract_version_json(&content, "version"),
                });
            }
        }
    }

    None
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
