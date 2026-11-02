// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native Tina4 AI-skills installer.
//!
//! This replaces the old shell-out. The client used to download
//! `install-skills.sh` / `install-skills.ps1` and run it — on Windows as
//! `powershell -Command "iex ([System.IO.File]::ReadAllText('<downloaded .ps1>'))"`.
//! A freshly-updated, low-reputation `tina4.exe` spawning PowerShell to `iex`
//! a just-downloaded script is exactly the shape Windows Defender / Attack
//! Surface Reduction blocks: a real `tina4 update` came back with
//! `os error 225` (ERROR_VIRUS_INFECTED) and installed no skills. The binary is
//! EV-signed and fine; the spawn was the problem.
//!
//! So the whole installer now runs in-process. It fetches each skill file with
//! the CLI's own download primitive (`crate::download_file_classified`),
//! verifies every file against the published `skills.sha256` manifest with
//! `ring`'s SHA-256, and writes the files into the target directories itself.
//! No `powershell`, no `sh`, no `cmd`, no `iex`, no `install-skills.*` — this
//! module spawns no process of its own (the download primitive it calls prefers
//! `curl`/`curl.exe`, the same one `tina4 update` already uses to fetch its own
//! binary, which Defender does not object to).
//!
//! The behaviour is a faithful port of both scripts: same sources and fallback
//! order, same `TINA4_SKILLS_*` environment variables, same target directories,
//! the same checksum gate, and the same atomic publish. The one deliberate
//! difference is the dead-host optimisation: `download_file_classified` reports
//! an HTTP error without distinguishing 4xx from 5xx, so a host is written off
//! for the rest of the run only on a transport failure (no answer / DNS /
//! connect / TLS), while any HTTP error moves to the next source for that one
//! file. The set of files that land, and where, is identical.

use std::fs;
use std::path::{Path, PathBuf};

use crate::console::{icon_ok, icon_play, icon_warn};
use colored::Colorize;

/// The skills ref used only as an OFFLINE fallback, when `TINA4_SKILLS_REF` is
/// unset AND the latest published ref cannot be fetched from the served installer
/// (see [`resolve_ref`]). The normal path resolves the ref dynamically, so a
/// skills release reaches `tina4 update` WITHOUT a CLI release. This constant is
/// just the last-resort floor for a fully offline machine.
const DEFAULT_REF: &str = "3.13.147";

/// The served installer whose pinned `TINA4_SKILLS_REF` default IS the current
/// published ref. `tina4 update` / `tina4 skills` resolve "latest" from here (the
/// same endpoint `tina4 doctor` reports against), so what a refresh installs
/// always equals what the published installer would. Fetched with the in-process
/// download primitive - this module still spawns nothing of its own.
const SKILLS_INSTALL_URL: &str = "https://tina4.com/install-skills.sh";

/// The three fetch tiers, in priority order. tina4.com FIRST so the common path
/// never depends on GitHub raw (which 503s during GitHub incidents); jsDelivr and
/// raw are automatic fallbacks. Each tier has its OWN path shape — see
/// [`skill_urls`] — so a tier is not just a swapped root prefix.
const DEFAULT_TINA4_ROOT: &str = "https://tina4.com/skills";
const DEFAULT_JSDELIVR_ROOT: &str = "https://cdn.jsdelivr.net/gh/tina4stack";
const DEFAULT_RAW_ROOT: &str = "https://raw.githubusercontent.com/tina4stack";

/// Attempts per source are `retry_count + 1`; `retry_delay` seconds between them.
/// The same defaults the scripts use.
const DEFAULT_RETRY_COUNT: u32 = 3;
const DEFAULT_RETRY_DELAY_SECS: u64 = 2;

/// A ceiling on the whole fetch walk, mirroring `fetch_skills_installer`'s
/// budget: the download primitive runs `curl` without a timeout, so a host that
/// accepts a connection and then says nothing can hang, and the retries would
/// multiply that wait. Once this much time has gone, no further attempt starts.
const FETCH_BUDGET_SECS: u64 = 60;

/// Skill directories from an older layout that must be removed from a target
/// before the current skills are written.
const LEGACY_SKILLS: &[&str] = &["tina4-developer"];

/// The checksum manifest of what a target currently has installed, written at
/// the end of every publish and read at the start of the next one. It is how a
/// refresh tells a file the user edited from a clean upstream file: a skill
/// whose on-disk bytes no longer match what we recorded installing is backed up
/// before it is replaced, instead of being overwritten. Dot-prefixed so coding
/// tools never scan it as a skill. Same `sha256sum` shape as `skills.sha256`.
const INSTALLED_MANIFEST: &str = ".tina4-skills.sha256";

/// Which framework repo hosts a skill, for the jsDelivr and raw FALLBACK URLs
/// only (the primary tina4.com tier is flat and repo-independent). This is the
/// ONE piece of per-skill knowledge left, and it is a rule, not a list, so new
/// skills need no change here: a per-language developer skill comes from its own
/// framework repo, `tina4-cli` from the CLI repo, and every other (shared) skill
/// is served from `tina4-python`. Getting this wrong for a brand-new skill only
/// loses the CDN fallback for it; the primary tier still serves it.
fn repo_for_skill(skill: &str) -> &'static str {
    match skill {
        "tina4-developer-python" => "tina4-python",
        "tina4-developer-php" => "tina4-php",
        "tina4-developer-ruby" => "tina4-ruby",
        "tina4-developer-nodejs" => "tina4-nodejs",
        "tina4-cli" => "tina4",
        _ => "tina4-python",
    }
}

/// The resolved configuration for one install run, read from the environment
/// once so every file is fetched from the same tiers at the same ref.
struct Config {
    reference: String,
    tina4_root: String,
    jsdelivr_root: String,
    raw_root: String,
    retry_count: u32,
    retry_delay: std::time::Duration,
}

impl Config {
    fn from_env() -> Self {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let num = |name: &str, default: u64| -> u64 {
            env(name).and_then(|v| v.parse().ok()).unwrap_or(default)
        };
        Config {
            reference: resolve_ref(),
            tina4_root: env("TINA4_SKILLS_TINA4_ROOT").unwrap_or_else(|| DEFAULT_TINA4_ROOT.to_string()),
            jsdelivr_root: env("TINA4_SKILLS_JSDELIVR_ROOT")
                .unwrap_or_else(|| DEFAULT_JSDELIVR_ROOT.to_string()),
            raw_root: env("TINA4_SKILLS_RAW_ROOT").unwrap_or_else(|| DEFAULT_RAW_ROOT.to_string()),
            retry_count: num("TINA4_SKILLS_RETRY_COUNT", DEFAULT_RETRY_COUNT as u64) as u32,
            retry_delay: std::time::Duration::from_secs(num(
                "TINA4_SKILLS_RETRY_DELAY",
                DEFAULT_RETRY_DELAY_SECS,
            )),
        }
    }

    /// Candidate URLs for one skill file, in priority order. Whitespace-safe:
    /// URLs never contain a space.
    fn skill_urls(&self, repo: &str, skill: &str, relative: &str) -> [String; 3] {
        [
            format!("{}/{}/{}/{}", self.tina4_root, self.reference, skill, relative),
            format!(
                "{}/{}@{}/.claude/skills/{}/{}",
                self.jsdelivr_root, repo, self.reference, skill, relative
            ),
            format!(
                "{}/{}/{}/.claude/skills/{}/{}",
                self.raw_root, repo, self.reference, skill, relative
            ),
        ]
    }

    /// Candidate URLs for the `skills.sha256` manifest, which lives in the
    /// `tina4` repo at this ref.
    fn manifest_urls(&self) -> [String; 3] {
        [
            format!("{}/{}/skills.sha256", self.tina4_root, self.reference),
            format!("{}/tina4@{}/skills.sha256", self.jsdelivr_root, self.reference),
            format!("{}/tina4/{}/skills.sha256", self.raw_root, self.reference),
        ]
    }
}

/// Resolve the skills ref to install. `TINA4_SKILLS_REF` always wins (reproducible
/// / pinned installs). Otherwise the current published ref is fetched from the
/// served installer, so `tina4 update` always lands the latest skills with no CLI
/// release. Only a fully offline machine (fetch fails) falls back to `DEFAULT_REF`.
fn resolve_ref() -> String {
    if let Some(pinned) = std::env::var("TINA4_SKILLS_REF").ok().filter(|v| !v.is_empty()) {
        return pinned;
    }
    fetch_latest_ref().unwrap_or_else(|| DEFAULT_REF.to_string())
}

/// Fetch the served installer in-process (no spawn of our own) and read its
/// pinned `TINA4_SKILLS_REF` default. `None` on any download or parse failure, so
/// the caller falls back to the offline floor.
fn fetch_latest_ref() -> Option<String> {
    let tmp = std::env::temp_dir()
        .join(format!("tina4-skillsref-{}", std::process::id()));
    let outcome = crate::download_file_classified(SKILLS_INSTALL_URL, &tmp);
    let parsed = match outcome {
        crate::DownloadOutcome::Ok => fs::read_to_string(&tmp).ok().and_then(|s| parse_installer_ref(&s)),
        _ => None,
    };
    let _ = fs::remove_file(&tmp);
    parsed
}

/// Extract the `X.Y.Z` from a shell line like `ref="${TINA4_SKILLS_REF:-3.13.147}"`
/// or a PowerShell `else { "3.13.147" }`. Reads the value after the marker up to
/// the first closing brace or quote. Must stay in sync with the installer shape.
fn parse_installer_ref(installer: &str) -> Option<String> {
    let marker = "TINA4_SKILLS_REF:-";
    let start = installer.find(marker)? + marker.len();
    let rest = &installer[start..];
    let end = rest.find(['}', '"', '\'', '\n'])?;
    let value = rest[..end].trim();
    if value.is_empty() || !value.contains('.') {
        return None;
    }
    Some(value.to_string())
}

/// The three destinations a target maps to, under `home`.
fn destinations_for_target(target: &str, home: &Path) -> Option<Vec<PathBuf>> {
    let claude = || home.join(".claude").join("skills");
    let codex = || home.join(".agents").join("skills");
    let cursor = || home.join(".cursor").join("skills");
    match target {
        "claude" => Some(vec![claude()]),
        "codex" => Some(vec![codex()]),
        "cursor" => Some(vec![cursor()]),
        "all" => Some(vec![claude(), codex(), cursor()]),
        _ => None,
    }
}

/// `$TINA4_SKILLS_HOME`, else the user's home directory.
fn skill_home() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TINA4_SKILLS_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Install the skills for `target` (`claude` / `codex` / `cursor` / `all`).
/// Returns `true` on a fully verified, published install; on any failure it
/// prints the reason and a "run later" hint and returns `false`, leaving every
/// existing skill directory untouched (nothing is published unless every file
/// downloaded and verified).
pub fn install(target: &str) -> bool {
    println!("  {} Installing tina4 AI skills for {}...", icon_play().green(), target);
    match install_inner(target) {
        Ok(()) => true,
        Err(why) => {
            println!("  {} {}", icon_warn().yellow(), why);
            println!(
                "  {} Skills install skipped — run later: {}",
                icon_warn().yellow(),
                "tina4 ai".cyan()
            );
            false
        }
    }
}

fn install_inner(target: &str) -> Result<(), String> {
    let home = skill_home()
        .ok_or_else(|| "Could not find your home directory (HOME / USERPROFILE unset).".to_string())?;
    let destinations = destinations_for_target(target, &home).ok_or_else(|| {
        "Choose one of: tina4 skills claude, tina4 skills codex, tina4 skills cursor, or tina4 skills all."
            .to_string()
    })?;

    let config = Config::from_env();
    println!("  Target: {}  (ref: {})", target, config.reference);

    let stage = Stage::new()
        .map_err(|e| format!("Could not create a temporary directory for the skills — {e}"))?;

    let mut dead_hosts: Vec<String> = Vec::new();
    let started = std::time::Instant::now();

    // The published `skills.sha256` manifest is the SOURCE OF TRUTH for which
    // files make up this ref. Fetch it first and derive the entire file set from
    // it, so the client never carries a hardcoded skill list that can drift from a
    // release (the drift that shipped an 8-skill installer against a 9-skill ref).
    let entries = fetch_manifest_entries(&config, &stage.path, &mut dead_hosts, started)?;

    // Fetch every file the manifest names, from the right tier. The first path
    // component is the skill; the rest is its path within the skill (SKILL.md, or
    // references/... nested to any depth). New skills, new references and deeper
    // nesting all flow through here with no code change.
    let mut skills_seen: Vec<String> = Vec::new();
    for (_hash, relative) in &entries {
        let (skill, within) = relative
            .split_once('/')
            .ok_or_else(|| format!("Manifest path has no skill component: {relative}"))?;
        let dest = stage.path.join(skill).join(within);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create the staging directory for {skill} — {e}"))?;
        }
        download_file(
            &config,
            &dest,
            &config.skill_urls(repo_for_skill(skill), skill, within),
            &mut dead_hosts,
            started,
        )?;
        if !skills_seen.iter().any(|s| s == skill) {
            skills_seen.push(skill.to_string());
            println!("  {} {}  ({})", "+".green(), skill, repo_for_skill(skill));
        }
    }

    // Verify every staged file against the manifest BEFORE anything is installed,
    // so a tampered or truncated download can never reach a skills directory.
    verify_entries(&entries, &stage.path, &config.reference)?;

    // Publish into every destination.
    for destination in &destinations {
        publish(&stage.path, destination, &config.reference, &entries)
            .map_err(|e| format!("Could not install the skills into {} — {e}", destination.display()))?;
        println!("  {} installed for {}", icon_ok().green(), destination.display());
    }

    println!(
        "  {} Done - {} skills installed for {} (ref {}). Restart your coding tool to pick them up.",
        icon_ok().green(),
        skills_seen.len(),
        target,
        config.reference
    );
    Ok(())
}

/// A private staging directory that removes itself when dropped, so a failed or
/// panicking run never leaves a half-downloaded tree behind.
struct Stage {
    path: PathBuf,
}

impl Stage {
    /// `create_dir` fails if the path already exists, so this can never adopt a
    /// directory another user planted in a world-writable temp location; the
    /// name carries the pid and a nanosecond timestamp so a recycled pid cannot
    /// collide. On Unix it is narrowed to 0700.
    fn new() -> std::io::Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("tina4-skills-{}-{}", std::process::id(), stamp));
        fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Stage { path })
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// What one fetch of one URL amounted to.
enum FetchResult {
    /// Got the file.
    Got,
    /// The server answered with an HTTP error (4xx/5xx). Says nothing about the
    /// next source, so move on to it for this file.
    NotHere,
    /// No HTTP answer at all (DNS, connect, TLS, timeout, transport). Equally
    /// true for every remaining file, so the host is worth writing off.
    HostDead,
}

/// `scheme://authority` for a URL, so two tiers pointing at the same host share
/// its fate. Falls back to the whole URL rather than an empty key, which would
/// collide with every other empty key.
fn host_of(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        let authority_end = after.find('/').unwrap_or(after.len());
        return url[..scheme_end + 3 + authority_end].to_string();
    }
    url.to_string()
}

/// One full retry walk against one URL. Retries only transport failures (the
/// download primitive cannot distinguish a 4xx from a 5xx, so an HTTP error is
/// treated as "not on this host" and handed straight to the next source).
fn fetch_one(config: &Config, url: &str, dest: &Path) -> FetchResult {
    let attempts = config.retry_count + 1;
    for attempt in 1..=attempts {
        match crate::download_file_classified(url, dest) {
            crate::DownloadOutcome::Ok => return FetchResult::Got,
            crate::DownloadOutcome::Http => {
                let _ = fs::remove_file(dest);
                return FetchResult::NotHere;
            }
            crate::DownloadOutcome::Local(_) => {
                let _ = fs::remove_file(dest);
                if attempt < attempts {
                    std::thread::sleep(config.retry_delay);
                }
            }
        }
    }
    FetchResult::HostDead
}

/// Fetch `dest` from `urls` in priority order. Two passes: the first skips hosts
/// already written off this run; the second tries exactly those skipped hosts,
/// so a source is never lost — a host is only skipped while another is still
/// worth trying.
fn download_file(
    config: &Config,
    dest: &Path,
    urls: &[String],
    dead_hosts: &mut Vec<String>,
    started: std::time::Instant,
) -> Result<(), String> {
    let budget = std::time::Duration::from_secs(FETCH_BUDGET_SECS);
    let mut skipped: Vec<&String> = Vec::new();
    for url in urls {
        let host = host_of(url);
        if dead_hosts.contains(&host) {
            skipped.push(url);
            continue;
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "gave up fetching {} after {}s",
                dest.display(),
                started.elapsed().as_secs()
            ));
        }
        match fetch_one(config, url, dest) {
            FetchResult::Got => return Ok(()),
            FetchResult::HostDead => {
                if !dead_hosts.contains(&host) {
                    dead_hosts.push(host.clone());
                }
                eprintln!(
                    "  {} {host} is not answering; using the next source for the rest of this run",
                    "!".yellow()
                );
            }
            FetchResult::NotHere => {
                eprintln!("  {} not served by {host}, trying next source", "!".yellow());
            }
        }
    }
    for url in skipped {
        if let FetchResult::Got = fetch_one(config, url, dest) {
            return Ok(());
        }
    }
    Err(format!("every download source failed for {}", dest.display()))
}

/// Download the published manifest and parse it into `(expected_hash, relative)`
/// entries - the authoritative list of every file in this skills ref. Every path
/// is checked component-by-component so it can never escape the stage. An empty
/// or malformed manifest aborts with nothing installed.
fn fetch_manifest_entries(
    config: &Config,
    stage: &Path,
    dead_hosts: &mut Vec<String>,
    started: std::time::Instant,
) -> Result<Vec<(String, String)>, String> {
    let manifest_path = stage.join(".skills.sha256");
    download_file(config, &manifest_path, &config.manifest_urls(), dead_hosts, started)?;
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Could not read the skills checksum manifest — {e}"))?;
    let _ = fs::remove_file(&manifest_path);

    let mut entries = Vec::new();
    for line in manifest.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (expected, relative) = parse_manifest_line(line)
            .ok_or_else(|| format!("Malformed line in the skills checksum manifest: {line}"))?;
        for component in relative.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(format!("Unsafe path in the skills checksum manifest: {relative}"));
            }
        }
        entries.push((expected.to_string(), relative.to_string()));
    }
    if entries.is_empty() {
        return Err("The skills checksum manifest is empty — refusing to install.".to_string());
    }
    Ok(entries)
}

/// Verify every staged file against its manifest hash. A mismatch or a missing
/// named file aborts with nothing installed. Paths are joined component by
/// component so this is correct on Windows and cannot escape the stage.
fn verify_entries(entries: &[(String, String)], stage: &Path, reference: &str) -> Result<(), String> {
    for (expected, relative) in entries {
        let mut file = stage.to_path_buf();
        for component in relative.split('/') {
            file.push(component);
        }
        let bytes = fs::read(&file).map_err(|_| {
            format!("A skill file named in the manifest was not downloaded: {relative}")
        })?;
        if !sha256_hex(&bytes).eq_ignore_ascii_case(expected) {
            return Err(format!(
                "A skill file failed checksum verification (tampering or a stale manifest) — nothing installed: {relative}"
            ));
        }
    }
    println!(
        "  {} verified {} skill files against skills.sha256 (ref {})",
        icon_ok().green(),
        entries.len(),
        reference
    );
    Ok(())
}

/// Parse a `sha256sum`-format line: 64 hex chars, whitespace, then the path.
/// Returns `(expected_hex, relative_path)`.
fn parse_manifest_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_end();
    let (hash, rest) = line.split_once(|c: char| c.is_ascii_whitespace())?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let relative = rest.trim_start();
    // sha256sum marks a binary read with a leading '*'; the manifest is text, but
    // tolerate it so a binary-mode manifest still verifies.
    let relative = relative.strip_prefix('*').unwrap_or(relative);
    if relative.is_empty() {
        return None;
    }
    Some((hash, relative))
}

/// Lowercase hex SHA-256 of `bytes`, via `ring` (already in the dependency tree
/// through rustls). Not the CLI's `sha256_of_file`, which shells out to
/// `Get-FileHash` through PowerShell on Windows — the exact spawn this module
/// exists to remove.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Publish every staged skill into `destination`, replacing each one atomically
/// and removing any legacy skill directories first. A skill the user has edited
/// since it was installed is backed up aside before it is replaced, so a refresh
/// never silently discards local changes (`entries` is the new ref's manifest,
/// recorded for the next refresh to compare against).
fn publish(
    stage: &Path,
    destination: &Path,
    reference: &str,
    entries: &[(String, String)],
) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;

    for legacy in LEGACY_SKILLS {
        let legacy_path = destination.join(legacy);
        if legacy_path.exists() {
            fs::remove_dir_all(&legacy_path)?;
            println!("  {} removed legacy {legacy}", "-".yellow());
        }
    }

    // What we recorded installing last time. Empty for a first install or a
    // target last written by a pre-backup client; with nothing recorded we
    // cannot tell an edit from a clean file, so we do not guess - we replace as
    // before and record the manifest below, so the NEXT refresh is precise.
    let recorded = read_recorded_manifest(destination);

    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        install_one_skill(&entry.path(), destination, &entry.file_name(), &recorded)?;
    }

    // Trimmed on read (see doctor::read_installed_skills_ref), so the trailing
    // newline is immaterial; kept to match install-skills.sh's marker.
    fs::write(destination.join(".tina4-skills-ref"), format!("{reference}\n"))?;
    // Record what we just installed so the next refresh can spot local edits.
    write_recorded_manifest(destination, entries)?;
    Ok(())
}

/// Stage one already-downloaded skill into `destination`: copy it beside the
/// target, back up the installed copy first if the user edited it, then swing
/// the new one into place with a rename so a reader never sees a half-written
/// skill.
fn install_one_skill(
    staged: &Path,
    destination: &Path,
    name: &std::ffi::OsStr,
    recorded: &std::collections::HashMap<String, String>,
) -> std::io::Result<()> {
    let skill = name.to_string_lossy();
    let replacement = destination.join(format!(".{skill}.tina4-new"));
    let installed = destination.join(name);
    if replacement.exists() {
        fs::remove_dir_all(&replacement)?;
    }
    copy_dir_all(staged, &replacement)?;
    if installed.exists() {
        // Preserve the user's work before the directory is removed: a skill whose
        // files no longer match what we recorded installing is backed up aside.
        if !recorded.is_empty() && skill_is_modified(&installed, &skill, recorded) {
            let backup = back_up_skill(&installed, destination, &skill)?;
            println!(
                "  {} kept your edited {skill} - backed up to {}",
                "~".yellow(),
                backup.display()
            );
        }
        fs::remove_dir_all(&installed)?;
    }
    fs::rename(&replacement, &installed)?;
    Ok(())
}

/// Read the recorded install manifest from `destination` into `relative -> hash`
/// (lowercased). A missing file (first install, or an older client that never
/// wrote one) yields an empty map, which the caller reads as "nothing recorded".
fn read_recorded_manifest(destination: &Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(text) = fs::read_to_string(destination.join(INSTALLED_MANIFEST)) {
        for line in text.lines() {
            if let Some((hash, relative)) = parse_manifest_line(line) {
                map.insert(relative.to_string(), hash.to_ascii_lowercase());
            }
        }
    }
    map
}

/// Write the manifest of what we just installed, in `skills.sha256` shape, so the
/// next refresh can tell an edited file from a clean one.
fn write_recorded_manifest(
    destination: &Path,
    entries: &[(String, String)],
) -> std::io::Result<()> {
    let mut body = String::new();
    for (hash, relative) in entries {
        body.push_str(hash);
        body.push_str("  ");
        body.push_str(relative);
        body.push('\n');
    }
    fs::write(destination.join(INSTALLED_MANIFEST), body)
}

/// True when the installed `skill` directory no longer matches what `recorded`
/// says we installed: a file was edited, deleted, or added. Only files the
/// manifest named for this skill count; the dot-files we write live beside the
/// skill directories, never inside them, so they are never seen here.
fn skill_is_modified(
    installed: &Path,
    skill: &str,
    recorded: &std::collections::HashMap<String, String>,
) -> bool {
    let prefix = format!("{skill}/");
    let mut recorded_here = 0usize;
    for (relative, hash) in recorded {
        let Some(within) = relative.strip_prefix(&prefix) else {
            continue;
        };
        recorded_here += 1;
        let mut path = installed.to_path_buf();
        for component in within.split('/') {
            path.push(component);
        }
        match fs::read(&path) {
            Ok(bytes) => {
                if !sha256_hex(&bytes).eq_ignore_ascii_case(hash) {
                    return true; // edited in place
                }
            }
            Err(_) => return true, // a recorded file was deleted
        }
    }
    // A file the user added under the skill (one the manifest never named) is an
    // edit too; the recorded files all matched above, so a count mismatch can
    // only be an addition.
    count_files(installed) != recorded_here
}

/// Count the files (not directories) in a tree. Used to notice a file the user
/// added to a skill that the recorded manifest never named.
fn count_files(dir: &Path) -> usize {
    let mut total = 0;
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => total += count_files(&entry.path()),
                Ok(_) => total += 1,
                Err(_) => {}
            }
        }
    }
    total
}

/// Copy the user's modified skill aside before it is replaced, so the edit is
/// recoverable. Dot-prefixed (coding tools skip it) and stamped so repeated
/// refreshes never clobber an earlier backup. Returns the backup path to report.
fn back_up_skill(installed: &Path, destination: &Path, skill: &str) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let backup = destination.join(format!(".{skill}.tina4-backup-{stamp}"));
    copy_dir_all(installed, &backup)?;
    Ok(backup)
}

/// Recursively copy a directory tree. The stdlib has no equivalent, and the
/// trees here are a handful of small files, so a plain recursion is the lean
/// choice over a new dependency.
fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_maps_to_its_directories() {
        let home = Path::new("/home/dev");
        assert_eq!(
            destinations_for_target("claude", home),
            Some(vec![home.join(".claude").join("skills")])
        );
        assert_eq!(
            destinations_for_target("codex", home),
            Some(vec![home.join(".agents").join("skills")])
        );
        assert_eq!(
            destinations_for_target("cursor", home),
            Some(vec![home.join(".cursor").join("skills")])
        );
        assert_eq!(
            destinations_for_target("all", home),
            Some(vec![
                home.join(".claude").join("skills"),
                home.join(".agents").join("skills"),
                home.join(".cursor").join("skills"),
            ])
        );
        assert_eq!(destinations_for_target("nonsense", home), None);
    }

    fn default_config() -> Config {
        Config {
            reference: DEFAULT_REF.to_string(),
            tina4_root: DEFAULT_TINA4_ROOT.to_string(),
            jsdelivr_root: DEFAULT_JSDELIVR_ROOT.to_string(),
            raw_root: DEFAULT_RAW_ROOT.to_string(),
            retry_count: DEFAULT_RETRY_COUNT,
            retry_delay: std::time::Duration::from_secs(0),
        }
    }

    /// The exact URL shapes each tier serves. These must match install-skills.sh
    /// (`skill_urls`) and install-skills.ps1 (`Get-Tina4SkillUrls`) byte for byte,
    /// or a native install fetches from a different place than the scripts.
    #[test]
    fn skill_urls_match_the_scripts_for_each_tier() {
        let config = default_config();
        let urls = config.skill_urls("tina4-php", "tina4-developer-php", "references/realtime.md");
        assert_eq!(
            urls[0],
            "https://tina4.com/skills/3.13.147/tina4-developer-php/references/realtime.md"
        );
        assert_eq!(
            urls[1],
            "https://cdn.jsdelivr.net/gh/tina4stack/tina4-php@3.13.147/.claude/skills/tina4-developer-php/references/realtime.md"
        );
        assert_eq!(
            urls[2],
            "https://raw.githubusercontent.com/tina4stack/tina4-php/3.13.147/.claude/skills/tina4-developer-php/references/realtime.md"
        );

        let skill_md = config.skill_urls("tina4-python", "tina4-architect", "SKILL.md");
        assert_eq!(skill_md[0], "https://tina4.com/skills/3.13.147/tina4-architect/SKILL.md");
    }

    #[test]
    fn manifest_urls_point_at_the_tina4_repo() {
        let config = default_config();
        let urls = config.manifest_urls();
        assert_eq!(urls[0], "https://tina4.com/skills/3.13.147/skills.sha256");
        assert_eq!(urls[1], "https://cdn.jsdelivr.net/gh/tina4stack/tina4@3.13.147/skills.sha256");
        assert_eq!(urls[2], "https://raw.githubusercontent.com/tina4stack/tina4/3.13.147/skills.sha256");
    }

    /// The installer must have a second host to fall back to. A single source is
    /// what a 503 from raw.githubusercontent.com once turned into a failed
    /// `tina4 update`; collapsing the tiers onto one host brings that back.
    #[test]
    fn the_fetch_tiers_span_more_than_one_host() {
        let config = default_config();
        let urls = config.skill_urls("tina4-python", "tina4-js", "SKILL.md");
        let hosts: std::collections::BTreeSet<String> =
            urls.iter().map(|u| host_of(u)).collect();
        assert!(hosts.len() > 1, "every tier resolves to one host: {urls:?}");
    }

    #[test]
    fn host_of_extracts_scheme_and_authority() {
        assert_eq!(host_of("https://tina4.com/skills/3.13.147/x"), "https://tina4.com");
        assert_eq!(
            host_of("https://cdn.jsdelivr.net/gh/tina4stack/a@1/b"),
            "https://cdn.jsdelivr.net"
        );
        assert_eq!(host_of("file:///tmp/fixtures/x"), "file://");
        assert_eq!(host_of("not-a-url"), "not-a-url");
    }

    /// The installer is manifest-driven: the skill is the FIRST path component and
    /// the rest is its path within the skill, nested to any depth. This is how new
    /// skills / references / deeper nesting flow through with no code change.
    #[test]
    fn manifest_paths_split_into_skill_and_within() {
        assert_eq!("tina4-cli/SKILL.md".split_once('/'), Some(("tina4-cli", "SKILL.md")));
        assert_eq!(
            "tina4-maintainer/references/checklists/signing.md".split_once('/'),
            Some(("tina4-maintainer", "references/checklists/signing.md"))
        );
        assert_eq!(
            "tina4-design/references/ui-guide.md".split_once('/'),
            Some(("tina4-design", "references/ui-guide.md"))
        );
    }

    /// Repo routing is a RULE, not a list, so a brand-new skill needs no change
    /// here - and getting it wrong only loses the CDN fallback, never the primary
    /// tier. Guards the drift that shipped an 8-skill installer against a 9-skill ref.
    #[test]
    fn repo_routing_covers_known_and_future_skills() {
        assert_eq!(repo_for_skill("tina4-developer-python"), "tina4-python");
        assert_eq!(repo_for_skill("tina4-developer-nodejs"), "tina4-nodejs");
        assert_eq!(repo_for_skill("tina4-cli"), "tina4");
        // Shared skills (js, maintainer, architect, design) and any FUTURE skill
        // default to the canonical host, tina4-python.
        assert_eq!(repo_for_skill("tina4-maintainer"), "tina4-python");
        assert_eq!(repo_for_skill("tina4-design"), "tina4-python");
        assert_eq!(repo_for_skill("tina4-brand-new-skill-nobody-added-yet"), "tina4-python");
    }

    /// The latest ref is read from the served `install-skills.sh` (the only thing
    /// `fetch_latest_ref` fetches), so the parser reads the shell `:-` default
    /// shape. A skills release thus reaches `tina4 update` with no CLI release.
    #[test]
    fn parse_installer_ref_reads_the_shell_default() {
        assert_eq!(
            parse_installer_ref("set -eu\nref=\"${TINA4_SKILLS_REF:-3.13.147}\"\ntarget=x"),
            Some("3.13.147".to_string())
        );
        assert_eq!(
            parse_installer_ref("ref=\"${TINA4_SKILLS_REF:-3.13.200}\""),
            Some("3.13.200".to_string())
        );
        // No marker, or a non-version/empty value, yields None (caller falls back).
        assert_eq!(parse_installer_ref("nothing here"), None);
        assert_eq!(parse_installer_ref("TINA4_SKILLS_REF:-}"), None);
    }

    /// SHA-256 known-answer tests from FIPS 180-4 / RFC 6234, so the verifier is
    /// proven correct against published vectors, not against itself.
    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    /// The manifest is `sha256sum` format: 64 hex, whitespace, path.
    #[test]
    fn manifest_lines_parse_and_reject_junk() {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            parse_manifest_line(&format!("{hash}  tina4-js/SKILL.md")),
            Some((hash, "tina4-js/SKILL.md"))
        );
        // A binary-mode marker is tolerated.
        assert_eq!(
            parse_manifest_line(&format!("{hash} *tina4-js/SKILL.md")),
            Some((hash, "tina4-js/SKILL.md"))
        );
        assert_eq!(parse_manifest_line("deadbeef  short-hash.md"), None);
        assert_eq!(parse_manifest_line("no-hash-here"), None);
        assert_eq!(parse_manifest_line(&format!("{hash}   ")), None);
    }

    /// The whole point of this module: the skills install path spawns no process
    /// of its own. `Command::new` anywhere in it would reintroduce exactly the
    /// PowerShell/`sh` spawn the Windows Defender block was about. (The shared
    /// download primitive it calls lives in main.rs; that is `curl`, which
    /// `tina4 update` already uses to fetch its own binary.)
    #[test]
    fn the_skills_module_spawns_no_process() {
        // Scan only the production code — everything before the test module, so
        // this test's own needle strings ("powershell", "iex") are not counted —
        // and strip line comments (doc + `//`) so the prose explaining what this
        // module AVOIDS does not trip the check either.
        let source = include_str!("skills.rs");
        let production = source.split("\n#[cfg(test)]").next().unwrap_or(source);
        let code: String = production
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("Command::new"),
            "the native skills installer must spawn no process"
        );
        assert!(
            !code.contains("powershell") && !code.contains("iex"),
            "the native skills installer must not reach for PowerShell or iex"
        );
    }

    /// A unique temp directory for a test, removed on drop so a panicking test
    /// leaves nothing behind.
    struct TmpDir(PathBuf);

    impl TmpDir {
        fn new(tag: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir()
                .join(format!("tina4-skills-test-{tag}-{}-{stamp}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            TmpDir(path)
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Stage one skill with a single `SKILL.md`, returning the stage path and the
    /// matching manifest entries so a test can `publish` it.
    fn stage_one_skill(root: &Path, skill: &str, body: &str) -> (PathBuf, Vec<(String, String)>) {
        let stage = root.join(format!("stage-{}", sha256_hex(body.as_bytes())));
        let skill_dir = stage.join(skill);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), body).unwrap();
        (
            stage,
            vec![(sha256_hex(body.as_bytes()), format!("{skill}/SKILL.md"))],
        )
    }

    /// The backup directory for `skill` under `dest`, if a refresh created one.
    fn find_backup(dest: &Path, skill: &str) -> Option<PathBuf> {
        let prefix = format!(".{skill}.tina4-backup-");
        fs::read_dir(dest).ok()?.flatten().find_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&prefix)
                .then(|| entry.path())
        })
    }

    #[test]
    fn unmodified_skill_refreshes_without_a_backup() {
        let tmp = TmpDir::new("clean");
        let dest = tmp.0.join("skills");

        let (stage1, entries1) = stage_one_skill(&tmp.0, "demo", "version one\n");
        publish(&stage1, &dest, "3.0.0", &entries1).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("demo/SKILL.md")).unwrap(),
            "version one\n"
        );
        assert!(dest.join(INSTALLED_MANIFEST).exists());

        // A clean upstream update - the user changed nothing - must not back up.
        let (stage2, entries2) = stage_one_skill(&tmp.0, "demo", "version two\n");
        publish(&stage2, &dest, "3.0.1", &entries2).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("demo/SKILL.md")).unwrap(),
            "version two\n"
        );
        assert!(
            find_backup(&dest, "demo").is_none(),
            "a clean file must not be backed up"
        );
    }

    #[test]
    fn edited_skill_is_backed_up_before_replacement() {
        let tmp = TmpDir::new("edited");
        let dest = tmp.0.join("skills");

        let (stage1, entries1) = stage_one_skill(&tmp.0, "demo", "original\n");
        publish(&stage1, &dest, "3.0.0", &entries1).unwrap();

        // The user edits the installed skill in place.
        fs::write(dest.join("demo/SKILL.md"), "my local edits\n").unwrap();

        let (stage2, entries2) = stage_one_skill(&tmp.0, "demo", "upstream v2\n");
        publish(&stage2, &dest, "3.0.1", &entries2).unwrap();

        // The new version is installed...
        assert_eq!(
            fs::read_to_string(dest.join("demo/SKILL.md")).unwrap(),
            "upstream v2\n"
        );
        // ...and the user's edit survives in a backup, not overwritten.
        let backup = find_backup(&dest, "demo").expect("a backup of the edited skill");
        assert_eq!(
            fs::read_to_string(backup.join("SKILL.md")).unwrap(),
            "my local edits\n"
        );
    }

    #[test]
    fn an_added_file_counts_as_a_local_edit() {
        let tmp = TmpDir::new("added");
        let dest = tmp.0.join("skills");

        let (stage1, entries1) = stage_one_skill(&tmp.0, "demo", "base\n");
        publish(&stage1, &dest, "3.0.0", &entries1).unwrap();

        // The user drops a file into the skill the manifest never named.
        fs::write(dest.join("demo/NOTES.md"), "my notes\n").unwrap();

        let (stage2, entries2) = stage_one_skill(&tmp.0, "demo", "base v2\n");
        publish(&stage2, &dest, "3.0.1", &entries2).unwrap();

        let backup = find_backup(&dest, "demo").expect("an added file is a local edit");
        assert_eq!(
            fs::read_to_string(backup.join("NOTES.md")).unwrap(),
            "my notes\n"
        );
    }

    #[test]
    fn a_target_with_no_recorded_manifest_records_one_without_guessing() {
        let tmp = TmpDir::new("legacy");
        let dest = tmp.0.join("skills");
        // A target an older, pre-backup client wrote: a skill on disk, no manifest.
        fs::create_dir_all(dest.join("demo")).unwrap();
        fs::write(dest.join("demo/SKILL.md"), "from an old client\n").unwrap();
        assert!(!dest.join(INSTALLED_MANIFEST).exists());

        let (stage, entries) = stage_one_skill(&tmp.0, "demo", "new\n");
        publish(&stage, &dest, "3.0.1", &entries).unwrap();

        // With nothing recorded we do not guess at edits (no backup), but we DO
        // record a manifest, so the next refresh is precise.
        assert!(find_backup(&dest, "demo").is_none());
        assert!(dest.join(INSTALLED_MANIFEST).exists());
        assert_eq!(
            fs::read_to_string(dest.join("demo/SKILL.md")).unwrap(),
            "new\n"
        );
    }
}
