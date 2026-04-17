use colored::Colorize;
use notify::event::{EventKind, ModifyKind};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use crate::console::icon_fail;

/// Path fragments whose events we always ignore. Server-side writes
/// to these locations cause reload loops on some filesystems (notably
/// overlayfs on Podman/distrobox, where `notify` falls back to polling
/// and re-reports unchanged files as Modify events).
const IGNORED_SUBSTRINGS: &[&str] = &[
    "/__pycache__/",
    "/.git/",
    "/.venv/",
    "/venv/",
    "/node_modules/",
    "/vendor/",
    "/dist/",
    "/target/",
    "/logs/",
    "/.tina4/",
];

/// File extensions whose events we always ignore.
const IGNORED_EXTENSIONS: &[&str] = &[
    "log", "db", "db-wal", "db-shm", "sqlite", "sqlite-journal",
    "tmp", "swp", "swo", "pyc", "pyo",
    "scss", // SCSS watcher owns these — the compiled .css triggers the reload
];

/// Filter out events that are not real source-file changes.
///
/// On overlayfs (Podman/distrobox) the `notify` crate falls back to
/// polling mode, which happily re-reports the same file as "modified"
/// every poll even when no process has touched it. We defend in layers:
///
///   1. Event-kind: only Create / Modify(data) / Remove count.
///      Metadata and Access events are dropped.
///   2. Path: ignore well-known noise paths (logs, caches, vcs, build).
///   3. Extension: ignore transient file types (.log, .db-wal, .pyc).
///   4. Real-mtime check (done by caller via `is_actually_modified`).
fn is_meaningful_event(event: &Event) -> bool {
    let kind_ok = matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Any
    );
    if !kind_ok {
        return false;
    }
    for path in &event.paths {
        let s = path.to_string_lossy();
        if IGNORED_SUBSTRINGS.iter().any(|sub| s.contains(sub)) {
            return false;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if IGNORED_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                return false;
            }
        }
    }
    true
}

/// Return the file's mtime if it exists. Used to filter out spurious
/// events where the filesystem layer re-reports an unchanged file.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Watch src/, migrations/, .env for changes and POST /__dev/api/reload
/// to the framework server so the browser reloads. Blocks forever.
pub fn watch_and_reload(port: u16) {
    let (tx, rx) = mpsc::channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(2));

    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, config).expect("Failed to create watcher");

    let dirs = ["src", "migrations"];
    for dir in &dirs {
        let p = Path::new(dir);
        if p.exists() {
            let _ = watcher.watch(p, RecursiveMode::Recursive);
        }
    }
    // Watch .env file
    let env_path = Path::new(".env");
    if env_path.exists() {
        let _ = watcher.watch(env_path, RecursiveMode::NonRecursive);
    }

    // Track last known mtime per file so we can drop spurious events.
    let mut mtimes: HashMap<PathBuf, SystemTime> = HashMap::new();
    let mut last_reload = Instant::now();
    let url = format!("http://127.0.0.1:{}/__dev/api/reload", port);

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                // Layer 1+2+3: event-kind / path / extension filter
                if !is_meaningful_event(&event) {
                    continue;
                }

                // Layer 4: real-mtime check. On overlayfs the watcher
                // polls and re-fires events for unchanged files; skip
                // if the mtime hasn't advanced since we last saw it.
                let mut any_changed = false;
                let mut changed_path: Option<PathBuf> = None;
                for p in &event.paths {
                    if let Some(mt) = file_mtime(p) {
                        match mtimes.get(p) {
                            Some(prev) if *prev == mt => continue,
                            _ => {
                                mtimes.insert(p.clone(), mt);
                                any_changed = true;
                                if changed_path.is_none() {
                                    changed_path = Some(p.clone());
                                }
                            }
                        }
                    } else {
                        // File doesn't exist (Remove event) — still meaningful
                        any_changed = true;
                        if changed_path.is_none() {
                            changed_path = Some(p.clone());
                        }
                    }
                }
                if !any_changed {
                    continue;
                }

                // Global debounce: coalesce bursts within 500ms
                if last_reload.elapsed() < Duration::from_millis(500) {
                    continue;
                }
                last_reload = Instant::now();

                let file = changed_path
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Determine type: css for .css/.scss, reload for everything else
                let reload_type = if file.ends_with(".css") || file.ends_with(".scss") {
                    "css"
                } else {
                    "reload"
                };

                // POST to the framework's reload endpoint
                let body = format!(
                    r#"{{"type":"{}","file":"{}"}}"#,
                    reload_type,
                    file.replace('\\', "/")
                );

                // Fire-and-forget HTTP POST (don't block the watcher)
                let url_clone = url.clone();
                std::thread::spawn(move || {
                    let _ = ureq_post(&url_clone, &body);
                });
            }
            Ok(Err(e)) => {
                // `notify` emitted an error event — log and continue.
                eprintln!("{} Watcher event error: {}", icon_fail().red(), e);
            }
            Err(e) => {
                eprintln!("{} Watcher channel closed: {}", icon_fail().red(), e);
                break;
            }
        }
    }
}

/// Simple blocking HTTP POST using std::net (no external HTTP crate needed).
fn ureq_post(url: &str, body: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    // Parse host:port from URL
    let url = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{}", path);

    let mut stream = TcpStream::connect(host_port).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .ok();

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, host_port, body.len(), body
    );

    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    Ok(())
}

/// Watch SCSS directory and recompile on changes. Blocks forever.
pub fn watch_scss(input_dir: &str, output_dir: &str, minify: bool) {
    let (tx, rx) = mpsc::channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(2));

    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, config).expect("Failed to create watcher");

    let input = Path::new(input_dir);
    if input.exists() {
        watcher
            .watch(input, RecursiveMode::Recursive)
            .expect("Failed to watch SCSS directory");
    }

    let mut last_compile = Instant::now();
    let mut mtimes: HashMap<PathBuf, SystemTime> = HashMap::new();

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                // Filter 1: event kind (skip Access, Metadata).
                // We check kind directly instead of is_meaningful_event()
                // because IGNORED_EXTENSIONS blocks .scss from the reload
                // watcher — here in the SCSS watcher we WANT .scss files.
                let kind_ok = matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(ModifyKind::Data(_))
                        | EventKind::Modify(ModifyKind::Name(_))
                        | EventKind::Modify(ModifyKind::Any)
                        | EventKind::Any
                );
                if !kind_ok {
                    continue;
                }

                // Filter 2: only .scss files
                let has_scss = event.paths.iter().any(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("scss"))
                        .unwrap_or(false)
                });
                if !has_scss {
                    continue;
                }

                // Filter 3: real mtime check (defeats overlayfs polling noise)
                let mut any_changed = false;
                for p in &event.paths {
                    if let Some(mt) = file_mtime(p) {
                        match mtimes.get(p) {
                            Some(prev) if *prev == mt => continue,
                            _ => {
                                mtimes.insert(p.clone(), mt);
                                any_changed = true;
                            }
                        }
                    } else {
                        any_changed = true; // Remove event
                    }
                }
                if !any_changed {
                    continue;
                }

                // Debounce: skip if less than 500ms since last compile
                if last_compile.elapsed() < Duration::from_millis(500) {
                    continue;
                }
                last_compile = Instant::now();

                let file = event
                    .paths
                    .first()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                println!(
                    "\n{} SCSS changed ({}) — recompiling...",
                    "♻".cyan(),
                    Path::new(&file)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                );
                crate::scss::compile_dir(input_dir, output_dir, minify);
            }
            Ok(Err(e)) => {
                eprintln!("{} SCSS watcher event error: {}", icon_fail().red(), e);
            }
            Err(e) => {
                eprintln!("{} SCSS watcher channel closed: {}", icon_fail().red(), e);
                break;
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────
//
// Regression guards for the file-watcher spurious-reload loop
// reported in tina4stack/tina4-book#129 on Fedora Linux under
// Podman/distrobox (overlayfs fallback to polling). Keep these
// thin — they assert behaviour, not implementation.

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, MetadataKind, RemoveKind};
    use std::path::PathBuf;

    fn ev(kind: EventKind, path: &str) -> Event {
        Event {
            kind,
            paths: vec![PathBuf::from(path)],
            attrs: Default::default(),
        }
    }

    #[test]
    fn modify_data_event_on_source_file_is_meaningful() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/src/routes/home.py",
        );
        assert!(is_meaningful_event(&e));
    }

    #[test]
    fn create_event_is_meaningful() {
        let e = ev(EventKind::Create(CreateKind::File), "/project/src/routes/new.py");
        assert!(is_meaningful_event(&e));
    }

    #[test]
    fn remove_event_is_meaningful() {
        let e = ev(
            EventKind::Remove(RemoveKind::File),
            "/project/src/routes/old.py",
        );
        assert!(is_meaningful_event(&e));
    }

    #[test]
    fn access_event_is_ignored() {
        // Overlayfs polling mode can emit spurious Access events on stat()
        let e = ev(
            EventKind::Access(AccessKind::Any),
            "/project/src/routes/home.py",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn metadata_only_event_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            "/project/src/routes/home.py",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn log_file_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/logs/tina4.log",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn sqlite_wal_file_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/data/app.db-wal",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn pycache_event_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/src/routes/__pycache__/home.cpython-313.pyc",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn git_internal_event_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/.git/HEAD",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn node_modules_event_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/node_modules/.package-lock.json",
        );
        assert!(!is_meaningful_event(&e));
    }

    #[test]
    fn swap_file_is_ignored() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/src/routes/.home.py.swp",
        );
        assert!(!is_meaningful_event(&e));
    }

    // ── SCSS watcher filter tests ──

    /// Helper: returns true if an event would pass the SCSS watcher's
    /// kind + extension filter (mirrors the logic in watch_scss).
    ///
    /// Note: the SCSS watcher uses `is_meaningful_event` for kind/path
    /// filtering but then does its OWN `.scss` extension check — it does
    /// not rely on IGNORED_EXTENSIONS (which intentionally blocks `.scss`
    /// from the *reload* watcher to prevent double-fire).
    fn would_trigger_scss_compile(event: &Event) -> bool {
        // Kind filter: same as is_meaningful_event but without the
        // extension check (SCSS watcher checks extension separately)
        let kind_ok = matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(ModifyKind::Data(_))
                | EventKind::Modify(ModifyKind::Name(_))
                | EventKind::Modify(ModifyKind::Any)
                | EventKind::Any
        );
        if !kind_ok {
            return false;
        }
        // Path substring filter (same as is_meaningful_event)
        for path in &event.paths {
            let s = path.to_string_lossy();
            if IGNORED_SUBSTRINGS.iter().any(|sub| s.contains(sub)) {
                return false;
            }
        }
        // SCSS watcher's own extension filter: only .scss
        event.paths.iter().any(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("scss"))
                .unwrap_or(false)
        })
    }

    #[test]
    fn scss_file_modify_triggers_compile() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/src/scss/main.scss",
        );
        assert!(would_trigger_scss_compile(&e));
    }

    #[test]
    fn scss_file_create_triggers_compile() {
        let e = ev(
            EventKind::Create(CreateKind::File),
            "/project/src/scss/_variables.scss",
        );
        assert!(would_trigger_scss_compile(&e));
    }

    #[test]
    fn non_scss_file_in_scss_dir_does_not_trigger() {
        let e = ev(
            EventKind::Create(CreateKind::File),
            "/project/src/scss/README.md",
        );
        assert!(!would_trigger_scss_compile(&e));
    }

    #[test]
    fn css_file_in_scss_dir_does_not_trigger() {
        let e = ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/project/src/scss/output.css",
        );
        assert!(!would_trigger_scss_compile(&e));
    }

    #[test]
    fn access_event_on_scss_does_not_trigger() {
        let e = ev(
            EventKind::Access(AccessKind::Any),
            "/project/src/scss/main.scss",
        );
        assert!(!would_trigger_scss_compile(&e));
    }

    #[test]
    fn py_file_in_src_does_not_trigger_scss() {
        let e = ev(
            EventKind::Create(CreateKind::File),
            "/project/src/routes/api.py",
        );
        assert!(!would_trigger_scss_compile(&e));
    }
}
