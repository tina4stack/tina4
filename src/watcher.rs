use colored::Colorize;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::console::icon_fail;

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
