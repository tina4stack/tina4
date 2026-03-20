use colored::Colorize;
use std::fs;
use std::path::Path;

/// Compile all non-partial SCSS files in `input_dir` to `output_dir`.
///
/// Partials (files starting with `_`) are skipped as top-level targets
/// but are resolved via `@import`.
pub fn compile_dir(input_dir: &str, output_dir: &str, minify: bool) {
    let input = Path::new(input_dir);
    if !input.exists() {
        return;
    }

    // Ensure output directory exists
    let output = Path::new(output_dir);
    if let Err(e) = fs::create_dir_all(output) {
        eprintln!("{} Cannot create {}: {}", "✗".red(), output_dir, e);
        return;
    }

    let entries = match fs::read_dir(input) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{} Cannot read {}: {}", "✗".red(), input_dir, e);
            return;
        }
    };

    let mut compiled = 0u32;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip partials, non-scss files, and directories
        if name.starts_with('_') || !name.ends_with(".scss") || path.is_dir() {
            continue;
        }

        match compile_file(&path, output, minify) {
            Ok(out_path) => {
                compiled += 1;
                println!(
                    "  {} {} → {}",
                    "✓".green(),
                    path.display().to_string().dimmed(),
                    out_path.cyan()
                );
            }
            Err(e) => {
                eprintln!(
                    "  {} {} — {}",
                    "✗".red(),
                    path.display(),
                    e
                );
            }
        }
    }

    if compiled > 0 {
        println!(
            "{} Compiled {} SCSS file{}",
            "✓".green(),
            compiled,
            if compiled == 1 { "" } else { "s" }
        );
    }
}

/// Compile a single SCSS file to CSS using the `grass` crate.
fn compile_file(
    scss_path: &Path,
    output_dir: &Path,
    minify: bool,
) -> Result<String, String> {
    let options = grass::Options::default()
        .style(if minify {
            grass::OutputStyle::Compressed
        } else {
            grass::OutputStyle::Expanded
        })
        .load_path(scss_path.parent().unwrap_or(Path::new(".")));

    let css = grass::from_path(scss_path, &options)
        .map_err(|e| format!("{}", e))?;

    // Output filename: foo.scss → foo.css
    let stem = scss_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let suffix = if minify { ".min.css" } else { ".css" };
    let out_name = format!("{}{}", stem, suffix);
    let out_path = output_dir.join(&out_name);

    fs::write(&out_path, &css)
        .map_err(|e| format!("Write failed: {}", e))?;

    Ok(out_path.display().to_string())
}

/// Compile a single SCSS string to CSS (used for testing / one-off).
#[allow(dead_code)]
pub fn compile_string(scss: &str, minify: bool) -> Result<String, String> {
    let options = grass::Options::default().style(if minify {
        grass::OutputStyle::Compressed
    } else {
        grass::OutputStyle::Expanded
    });

    grass::from_string(scss.to_string(), &options).map_err(|e| format!("{}", e))
}
