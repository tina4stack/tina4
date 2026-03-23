use colored::Colorize;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::console::{icon_fail, icon_ok, icon_play};
use crate::detect::ProjectInfo;

/// Watch SCSS directory and recompile on changes. Blocks forever.
pub fn watch_scss(input_dir: &str, output_dir: &str, minify: bool) {
    let (tx, rx) = mpsc::channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(1));

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

/// Watch src/, migrations/, .env for changes.
/// On SCSS changes: recompile. On code changes: restart the server.
pub fn watch_and_reload(
    scss_dir: &str,
    css_dir: &str,
    info: &ProjectInfo,
    port: u16,
    host: &str,
    server: &mut std::process::Child,
) {
    let (tx, rx) = mpsc::channel();
    let config = Config::default().with_poll_interval(Duration::from_secs(1));

    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, config).expect("Failed to create watcher");

    // Watch directories that exist
    let watch_paths = ["src", "migrations"];
    for p in &watch_paths {
        let path = Path::new(p);
        if path.exists() {
            let _ = watcher.watch(path, RecursiveMode::Recursive);
        }
    }

    // Watch .env
    let env_path = Path::new(".env");
    if env_path.exists() {
        let _ = watcher.watch(env_path, RecursiveMode::NonRecursive);
    }

    let mut last_event = Instant::now();

    // Handle Ctrl+C gracefully
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(r);

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(event)) => {
                // Debounce
                if last_event.elapsed() < Duration::from_millis(500) {
                    continue;
                }
                last_event = Instant::now();

                let paths: Vec<String> = event
                    .paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();

                let is_scss = paths
                    .iter()
                    .any(|p| p.ends_with(".scss"));

                if is_scss {
                    println!("{} SCSS changed — recompiling", "♻".cyan());
                    crate::scss::compile_dir(scss_dir, css_dir, false);
                } else {
                    let changed = paths
                        .first()
                        .map(|p| p.as_str())
                        .unwrap_or("file");
                    println!(
                        "{} {} changed — restarting server...",
                        "♻".cyan(),
                        changed.dimmed()
                    );

                    // Kill old server, start new one
                    let _ = server.kill();
                    let _ = server.wait();

                    match crate::start_language_server(info, port, host) {
                        Some(child) => *server = child,
                        None => {
                            eprintln!("{} Failed to restart server", icon_fail().red());
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("{} Watch error: {:?}", icon_fail().red(), e);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Cleanup
    println!("\n{} Shutting down...", icon_play().yellow());
    let _ = server.kill();
    let _ = server.wait();
    println!("{} Server stopped", icon_ok().green());
}

fn ctrlc_handler(running: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        running.store(false, std::sync::atomic::Ordering::Relaxed);
    });
}
