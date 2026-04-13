use colored::Colorize;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::console::icon_fail;

/// Watch src/, migrations/, .env for changes and POST /__dev/api/reload
/// to the framework server so the browser reloads. Blocks forever.
pub fn watch_and_reload(port: u16) {
    let (tx, rx) = mpsc::channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(1));

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

    let mut last_reload = Instant::now();
    let url = format!("http://127.0.0.1:{}/__dev/api/reload", port);

    loop {
        match rx.recv() {
            Ok(event) => {
                // Debounce: skip if less than 500ms since last reload
                if last_reload.elapsed() < Duration::from_millis(500) {
                    continue;
                }
                last_reload = Instant::now();

                // Extract changed file path from event
                let file = event
                    .ok()
                    .and_then(|e| e.paths.first().cloned())
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
            Err(e) => {
                eprintln!("{} Watcher error: {}", icon_fail().red(), e);
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

    loop {
        match rx.recv() {
            Ok(_event) => {
                // Debounce: skip if less than 500ms since last compile
                if last_compile.elapsed() < Duration::from_millis(500) {
                    continue;
                }
                last_compile = Instant::now();

                println!(
                    "\n{} SCSS changed — recompiling...",
                    "♻".cyan()
                );
                crate::scss::compile_dir(input_dir, output_dir, minify);
            }
            Err(e) => {
                eprintln!("{} Watcher error: {}", icon_fail().red(), e);
                break;
            }
        }
    }
}
