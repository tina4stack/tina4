//! Consume the framework CLI's self-describing `commands --json` manifest.
//!
//! Phase 2 of the self-describing-client epic: instead of hardcoding which
//! commands each framework accepts, the client ASKS the detected framework CLI
//! what it supports (`<cli> commands --json`) and renders that in `tina4 --help`.
//!
//! This is consumed for the HELP LISTING ONLY. Dispatch never touches it — the
//! native conductor set is client-owned and every other command is forwarded
//! blind (the framework rejects unknowns). So a manifest miss can never break a
//! command; the worst case is a slightly-stale help listing for one run.
//!
//! The manifest is cached at `.tina4/commands.json` (gitignored) keyed by a
//! cheap stat fingerprint (path + mtime + size) of the resolved framework CLI:
//! an unchanged fingerprint reuses the cache without spawning; an upgrade OR a
//! local editable-install edit changes it and forces a re-query. `--refresh`
//! bypasses the cache. Any spawn/parse failure falls back silently (`None`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::detect::ProjectInfo;

/// One command in the manifest. `args` / `subcommands` are only present when the
/// framework advertises them, so both default to empty and are omitted on write.
#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subcommands: Vec<String>,
}

/// The `{framework, version, commands[]}` shape emitted by every framework's
/// `commands --json`. Identical across Python, PHP, Ruby, and Node.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub version: String,
    pub commands: Vec<Command>,
}

/// On-disk cache read shape: the manifest plus the fingerprint of the CLI it was
/// read from.
#[derive(Deserialize)]
struct Cache {
    fingerprint: String,
    manifest: Manifest,
}

/// On-disk cache write shape — borrows to avoid cloning the manifest.
#[derive(Serialize)]
struct CacheRef<'a> {
    fingerprint: &'a str,
    manifest: &'a Manifest,
}

/// Where the per-project manifest cache lives (relative to the project root).
fn cache_path() -> PathBuf {
    Path::new(".tina4").join("commands.json")
}

/// Load the manifest for the detected project: reuse a fingerprint-valid cache,
/// otherwise query the framework CLI and rewrite the cache. Returns `None` on
/// any failure so callers fall back to built-in behavior.
pub fn load(info: &ProjectInfo, refresh: bool) -> Option<Manifest> {
    let path = cache_path();
    let fingerprint = fingerprint(info);

    // Fast path: an unchanged fingerprint means the CLI hasn't moved — reuse the
    // cached manifest without spawning anything.
    if !refresh {
        if let Some(fp) = fingerprint.as_deref() {
            if let Some(manifest) = read_valid_cache(&path, fp) {
                return Some(manifest);
            }
        }
    }

    // Slow path: ask the framework CLI. Any failure => graceful fallback.
    let manifest = query(info)?;

    // Persist keyed by the current fingerprint (best-effort — a write failure
    // just means we re-query next run, never a broken command).
    if let Some(fp) = fingerprint.as_deref() {
        write_cache(&path, fp, &manifest);
    }
    Some(manifest)
}

/// Spawn `<framework-cli> commands --json` and parse the manifest. `None` on a
/// spawn error, a non-zero exit, OR output that isn't the expected JSON — the
/// last case covers an older CLI that predates `commands` (it prints a
/// "Unknown command" help blob instead of a manifest).
fn query(info: &ProjectInfo) -> Option<Manifest> {
    let (command, mut args) = crate::resolve_cli(info);
    args.push("commands".to_string());
    args.push("--json".to_string());
    let output = std::process::Command::new(&command)
        .args(&args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<Manifest>(&output.stdout).ok()
}

/// Read the cache and return its manifest only if the stored fingerprint matches
/// the expected one (a mismatch means the CLI changed and the cache is stale).
fn read_valid_cache(path: &Path, expected_fingerprint: &str) -> Option<Manifest> {
    let bytes = std::fs::read(path).ok()?;
    let cache: Cache = serde_json::from_slice(&bytes).ok()?;
    if cache.fingerprint == expected_fingerprint {
        Some(cache.manifest)
    } else {
        None
    }
}

/// Write the manifest + fingerprint to the cache. Best-effort: any I/O error is
/// ignored (dispatch never depends on this cache).
fn write_cache(path: &Path, fingerprint: &str, manifest: &Manifest) {
    let cache = CacheRef {
        fingerprint,
        manifest,
    };
    if let Ok(json) = serde_json::to_vec_pretty(&cache) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, json);
    }
}

/// Cheap stat fingerprint of the resolved framework CLI: `path|mtime|size`.
/// Catches an upgrade (size/mtime move) AND a local editable-install edit that a
/// version string would miss. `None` if no concrete CLI file can be located.
fn fingerprint(info: &ProjectInfo) -> Option<String> {
    fingerprint_of(&framework_cli_path(info)?)
}

fn fingerprint_of(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(format!("{}|{}|{}", path.display(), mtime, meta.len()))
}

/// Locate the concrete framework CLI file to stat for the fingerprint. Prefers
/// the project-local install (`.venv/bin`, `vendor/bin`, `node_modules/.bin`,
/// `bin/`) so editable installs invalidate correctly, then falls back to a
/// global binary on PATH. Distinct from `resolve_cli` (which returns the launcher
/// used to RUN the CLI, e.g. `uv run` / `npx` / `bundle exec`).
fn framework_cli_path(info: &ProjectInfo) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    match info.language.as_str() {
        "python" => candidates.push(PathBuf::from(if cfg!(windows) {
            r".venv\Scripts\tina4python.exe"
        } else {
            ".venv/bin/tina4python"
        })),
        "php" => {
            candidates.push(PathBuf::from(crate::console::php_vendor_bin("tina4php")));
            candidates.push(PathBuf::from("bin/tina4php"));
        }
        "ruby" => candidates.push(PathBuf::from("bin/tina4ruby")),
        "nodejs" => candidates.push(PathBuf::from(if cfg!(windows) {
            r"node_modules\.bin\tina4nodejs.cmd"
        } else {
            "node_modules/.bin/tina4nodejs"
        })),
        _ => {}
    }
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let global = match info.language.as_str() {
        "python" => "tina4python",
        "php" => "tina4php",
        "ruby" => "tina4ruby",
        "nodejs" => "tina4nodejs",
        _ => return None,
    };
    which::which(global).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tina4-manifest-{}-{}-{}", std::process::id(), name, n))
    }

    #[test]
    fn fingerprint_is_stable_and_tracks_size() {
        // Real file, real stat — no mocks. Same bytes => same fingerprint;
        // changed bytes (size differs) => different fingerprint.
        let file = unique("fp");
        std::fs::write(&file, b"one").unwrap();
        let a = fingerprint_of(&file).unwrap();
        assert_eq!(a, fingerprint_of(&file).unwrap(), "same file, same fingerprint");
        std::fs::write(&file, b"one-plus-a-lot-more-bytes").unwrap();
        assert_ne!(a, fingerprint_of(&file).unwrap(), "changed file, changed fingerprint");
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn fingerprint_none_for_missing_file() {
        assert!(fingerprint_of(Path::new("/no/such/tina4/cli/binary")).is_none());
    }

    #[test]
    fn cache_roundtrips_and_gates_on_fingerprint() {
        let dir = unique("cache");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        let manifest = Manifest {
            framework: "python".into(),
            version: "9.9.9".into(),
            commands: vec![Command {
                name: "migrate:create".into(),
                summary: "Create a new migration file".into(),
                args: vec!["description".into()],
                subcommands: vec![],
            }],
        };
        write_cache(&path, "FP-A", &manifest);

        // Matching fingerprint => returns the cached manifest.
        let got = read_valid_cache(&path, "FP-A").expect("cache hit on matching fingerprint");
        assert_eq!(got.version, "9.9.9");
        assert_eq!(got.commands[0].name, "migrate:create");
        assert_eq!(got.commands[0].args, vec!["description".to_string()]);

        // Mismatched fingerprint => rejected, forcing a re-query.
        assert!(read_valid_cache(&path, "FP-B").is_none(), "stale fingerprint must be rejected");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_valid_cache_none_on_missing_or_garbage() {
        let dir = unique("garbage");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_valid_cache(&dir.join("absent.json"), "x").is_none());
        let garbage = dir.join("garbage.json");
        std::fs::write(&garbage, b"this is not json").unwrap();
        assert!(read_valid_cache(&garbage, "x").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_parses_with_optional_fields_absent() {
        // The wire shape omits args/subcommands when empty — they must default.
        let json = r#"{"framework":"php","version":"3.0.0","commands":[
            {"name":"serve","summary":"Start dev server"},
            {"name":"generate","summary":"Scaffold","subcommands":["model","crud"]}
        ]}"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.framework, "php");
        assert!(manifest.commands[0].args.is_empty());
        assert!(manifest.commands[0].subcommands.is_empty());
        assert_eq!(
            manifest.commands[1].subcommands,
            vec!["model".to_string(), "crud".to_string()]
        );
    }
}
