// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use colored::Colorize;
use std::process::Command;

use crate::console::{self, icon_fail, icon_info, icon_ok, icon_play, icon_warn};

pub fn run(lang: &str) {
    let lang_norm = lang.to_lowercase();

    // Python is the toolchain-free path: uv installs uv + Python + tina4-python
    // into the user profile with no package manager and (on macOS) no Xcode
    // Command Line Tools. Every other runtime installs via Homebrew (macOS) /
    // Chocolatey (Windows) — and on macOS Homebrew, git, and the runtimes
    // themselves all need the Command Line Tools. So for the non-Python
    // languages: make sure the CLT are present first (kicking off their
    // installer + bailing cleanly if not), then bootstrap the package manager.
    // Python skips both, so a bare Mac with no Xcode can still build.
    if !matches!(lang_norm.as_str(), "python" | "py") {
        if !ensure_macos_build_tools() {
            return; // CLT installer launched + guidance printed — user re-runs.
        }
        ensure_package_manager();
    }

    match lang_norm.as_str() {
        "python" | "py" => install_python(),
        "php" => install_php(),
        "ruby" | "rb" => install_ruby(),
        "nodejs" | "node" | "js" => install_nodejs(),
        "tina4-js" | "tina4js" | "js-frontend" => install_tina4_js(),
        "codeql" => install_codeql(),
        "all" => {
            install_python();
            install_php();
            install_ruby();
            install_nodejs();
        }
        _ => {
            eprintln!(
                "{} Unknown target: {}. Use: python, php, ruby, nodejs, tina4-js, codeql, all",
                icon_fail().red(),
                lang
            );
            std::process::exit(1);
        }
    }
}

fn install_python() {
    println!("\n{} Installing Python...", icon_play().green());

    // Windows: go fully admin-free. uv installs to the user profile with no
    // Administrator and can install Python ITSELF (`uv python install`), so we
    // do NOT use Chocolatey for Python here — choco's python package writes
    // machine-wide and needs admin, which is exactly what broke the elevated
    // setup flow (installs landed in the admin profile, not the user's).
    if console::is_windows() {
        install_uv_windows();
        if check_exists("python3") || check_exists("python") {
            println!("  {} Python already installed", icon_ok().green());
        } else if check_exists("uv") {
            // uv-managed Python — downloaded into the user profile, no admin.
            println!("  {} Installing Python via uv (no admin needed)...", icon_play().green());
            // `--default` makes uv also create `python` / `python3` executables in
            // its bin dir (on PATH), so a bare `python` resolves afterward. Without
            // it uv installs a MANAGED interpreter that only `uv run` can see, which
            // reads as "uv installed but not python". Harmless no-op on an older uv.
            let _ = Command::new("uv")
                .args(["python", "install", "3.12", "--default"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();
        }
        // tina4python via uv tool — also user-level.
        install_tina4_cli("tina4python", "uv", &["tool", "install", "tina4-python"]);
        return;
    }

    // Get a usable Python WITHOUT pulling in the Xcode Command Line Tools on
    // macOS. uv is a prebuilt static binary and `uv python install` downloads a
    // standalone Python — both user-level, no compiler, no CLT. We deliberately
    // skip Homebrew and the system /usr/bin/python3 (a fresh Mac's python3 is
    // only a CLT stub that prompts to install the tools the moment it runs).
    if cfg!(target_os = "macos") {
        ensure_uv_unix();
        if check_exists("uv") {
            println!(
                "  {} Installing a managed Python via uv (no Xcode tools needed)...",
                icon_play().green()
            );
            // `--default` makes uv also create `python` / `python3` executables in
            // its bin dir (on PATH), so a bare `python` resolves afterward. Without
            // it uv installs a MANAGED interpreter that only `uv run` can see, which
            // reads as "uv installed but not python". Harmless no-op on an older uv.
            let _ = Command::new("uv")
                .args(["python", "install", "3.12", "--default"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();
        }
    } else if !install_python_linux_family() {
        return;
    }

    // Install tina4python (user-level uv tool — pure Python, no build step).
    install_tina4_cli("tina4python", "uv", &["tool", "install", "tina4-python"]);

    if console::is_android() {
        termux_local_bin_on_path();
    }
}

/// uv tool puts tina4python in ~/.local/bin. Most Linux shell profiles add that
/// directory to PATH; Termux's never does, so the tool would stay unreachable.
/// `uv tool update-shell` adds it to the shell startup files (Termux's
/// etc/profile still sources ~/.bashrc, so nothing else changes). New sessions
/// see it; the current shell does not.
fn termux_local_bin_on_path() {
    let Ok(home) = std::env::var("HOME") else { return };
    let bin = format!("{home}/.local/bin");
    let original = std::env::var("PATH").unwrap_or_default();
    if original.split(':').any(|p| p == bin) {
        return;
    }
    let ok = Command::new(crate::console::resolve_cmd("uv"))
        .args(["tool", "update-shell"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!(
            "  {} Added {} to PATH - open a new Termux session to use tina4python",
            icon_info().blue(),
            bin
        );
    }
}

/// Termux: no sudo, and apt's python3.12 package names do not exist. Termux
/// packages both Python and uv (built against Bionic) - use them. The astral.sh
/// installer has no Android build, and `uv python install` has no Android
/// interpreters to download, so neither is attempted here.
fn install_python_termux() {
    if !(check_exists("python3") || check_exists("python")) || !check_exists("uv") {
        termux_install(termux_packages("python"));
    } else {
        println!("  {} Python and uv already installed", icon_ok().green());
    }
}

/// Python on iSH, Termux or a Linux distro. True when tina4python should then
/// be installed with uv; iSH takes no uv, so it stops here.
fn install_python_linux_family() -> bool {
    if console::is_ish() {
        install_python_ish();
        false
    } else if console::is_android() {
        install_python_termux();
        true
    } else {
        install_python_linux()
    }
}

/// iSH (iOS): Alpine's python3 only. No uv and no global tina4python - uv
/// crashes under iSH, and each project gets tina4-python in its own .venv from
/// `tina4 init`, which is where `tina4 serve` runs it from.
fn install_python_ish() {
    if ish_install_runtime("python") {
        println!(
            "  {} Projects get tina4-python in their own .venv: tina4 init python <name>",
            icon_info().blue()
        );
    }
}

/// iSH (iOS): print why a language cannot run there. True when it cannot.
pub fn refused_on_ish(lang: &str) -> bool {
    match ish_unsupported(lang) {
        Some(why) if console::is_ish() => {
            eprintln!("  {} Not available on iSH (iOS): {}.", icon_fail().red(), why);
            true
        }
        _ => false,
    }
}

/// Linux: the distro's Python (apk or apt), then uv on top. False when nothing
/// after this can work, so tina4python is not attempted.
fn install_python_linux() -> bool {
    if check_exists("python3") || check_exists("python") {
        println!("  {} Python already installed", icon_ok().green());
    } else if check_exists("apk") {
        // Alpine - including iSH on iOS, which runs as root with no sudo.
        // python3 ONLY: `apk add` is all-or-nothing, and older Alpine
        // releases (iSH's App Store snapshot is v3.14) have no `uv`
        // package, so asking for both would install neither. uv is
        // added best-effort here and otherwise by ensure_uv_unix below.
        run_install_commands(&[
            ("apk", &["add", "python3"]),
            ("sudo", &["apk", "add", "python3"]),
        ]);
        let _ = Command::new("apk")
            .args(["add", "uv"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    } else {
        run_install_commands(&[(
            "sudo",
            &["apt-get", "install", "-y", "python3.12", "python3.12-venv"],
        )]);
    }
    // Checked whether Python was just installed or already there. Elsewhere
    // uv can fetch its own 3.12+ for tina4python, so only warn; on 32-bit
    // x86 (iSH) uv itself dies with SIGILL, so nothing below can work.
    let too_old = check_exists("apk") && python_too_old();
    if too_old && cfg!(target_arch = "x86") {
        return false;
    }
    ensure_uv_unix();
    true
}

/// Install uv (the Python package manager) on macOS/Linux via the official
/// installer. uv ships as a prebuilt static binary dropped into the user
/// profile (`~/.local/bin`) — no compiler and no Xcode Command Line Tools — then
/// spliced onto PATH for this process so the following `uv` calls resolve
/// without opening a new shell.
fn ensure_uv_unix() {
    if check_exists("uv") {
        println!("  {} uv already installed", icon_ok().green());
        return;
    }
    let Some(dl) = crate::console::unix_downloader() else {
        eprintln!("  {} Cannot install uv: {}", icon_fail().red(), crate::console::NO_DOWNLOADER);
        return;
    };
    println!("  {} Installing uv...", icon_play().green());
    let status = Command::new("sh")
        .args([
            "-c",
            &format!("{} | sh", dl.stdout_command("https://astral.sh/uv/install.sh")),
        ])
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    // A pipeline's status is its LAST command's, so a failed download still
    // "succeeds": `sh` reads nothing and exits 0. Believe only a uv on PATH.
    refresh_uv_path_unix();
    match status {
        Ok(_) if check_exists("uv") => println!("  {} uv installed", icon_ok().green()),
        Ok(s) => eprintln!("  {} uv was not installed (installer exited with {})", icon_fail().red(), s),
        Err(e) => eprintln!("  {} Failed to launch shell: {}", icon_fail().red(), e),
    }
}

/// Install uv on Windows with NO Administrator rights. The official installer
/// (`irm https://astral.sh/uv/install.ps1 | iex`) drops uv into the user profile
/// (~/.local/bin) — no admin, no Chocolatey. On a vanilla box (no Python yet)
/// this is the only viable route; when Python already exists, `pip install uv`
/// is a fast fallback. Either way uv ends up callable in THIS process via a
/// PATH splice, so the following `uv python install` / `uv tool install` work
/// without opening a new shell.
fn install_uv_windows() {
    if check_exists("uv") {
        println!("  {} uv already installed", icon_ok().green());
        return;
    }
    println!("  {} Installing uv (no admin needed)...", icon_play().green());
    // Primary: the official user-level installer (works with no Python present).
    let _ = Command::new("powershell")
        .args([
            "-ExecutionPolicy", "ByPass",
            "-NoProfile",
            "-Command", "irm https://astral.sh/uv/install.ps1 | iex",
        ])
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    refresh_uv_path_windows();
    // Fallback: if Python is already around, pip can deliver uv too.
    if !check_exists("uv") && (check_exists("python") || check_exists("python3")) {
        let py = if check_exists("python") { "python" } else { "python3" };
        let _ = Command::new(py)
            .args(["-m", "pip", "install", "--upgrade", "uv"])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        add_python_scripts_to_path_windows(py);
    }
    if check_exists("uv") {
        println!("  {} uv installed", icon_ok().green());
    } else {
        eprintln!(
            "  {} uv not installed — open a new terminal and run: irm https://astral.sh/uv/install.ps1 | iex",
            icon_fail().red()
        );
    }
}

/// Splice uv's known install directory into the current process's PATH on
/// Windows. Without this, `which::which("uv")` in this same process can't
/// find the just-installed binary because Windows env-var changes don't
/// propagate to running processes — the user would have to re-run
/// `tina4 install python` in a new shell to get tina4python installed.
fn refresh_uv_path_windows() {
    let Ok(home) = std::env::var("USERPROFILE") else { return };
    // uv's official Windows installer puts uv.exe under %USERPROFILE%\.local\bin
    // (newer) or %USERPROFILE%\.cargo\bin (older). Add both — duplicates in
    // PATH are harmless.
    let candidates = [
        format!("{home}\\.local\\bin"),
        format!("{home}\\.cargo\\bin"),
    ];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current.split(';').map(|s| s.to_string()).collect();
    for c in &candidates {
        if std::path::Path::new(c).exists() && !parts.iter().any(|p| p.eq_ignore_ascii_case(c)) {
            parts.insert(0, c.clone());
        }
    }
    std::env::set_var("PATH", parts.join(";"));
}

/// Ask Python where its console-script shims (e.g. uv.exe from `pip install uv`)
/// live, and splice that directory into this process's PATH so
/// `which::which("uv")` resolves it without the user opening a new shell.
fn add_python_scripts_to_path_windows(py: &str) {
    let Ok(out) = Command::new(py)
        .args(["-c", "import sysconfig; print(sysconfig.get_path('scripts'))"])
        .output()
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dir.is_empty() || !std::path::Path::new(&dir).exists() {
        return;
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current.split(';').map(|s| s.to_string()).collect();
    if !parts.iter().any(|p| p.eq_ignore_ascii_case(&dir)) {
        parts.insert(0, dir);
        std::env::set_var("PATH", parts.join(";"));
    }
}

/// Splice ~/.local/bin and ~/.cargo/bin into PATH on Unix for the same reason
/// as refresh_uv_path_windows — `which::which` only searches the current
/// process's PATH, which doesn't yet include uv's install location.
fn refresh_uv_path_unix() {
    let Ok(home) = std::env::var("HOME") else { return };
    let candidates = [format!("{home}/.local/bin"), format!("{home}/.cargo/bin")];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current.split(':').map(|s| s.to_string()).collect();
    for c in &candidates {
        if std::path::Path::new(c).exists() && !parts.iter().any(|p| p == c) {
            parts.insert(0, c.clone());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

fn install_php() {
    println!("\n{} Installing PHP...", icon_play().green());
    if refused_on_ish("php") {
        return;
    }

    if check_exists("php") {
        println!("  {} PHP already installed", icon_ok().green());
    } else if console::is_android() {
        termux_install(termux_packages("php"));
    } else {
        run_install_commands(&[
            ("choco", &["install", "php", "-y"]),
            ("brew", &["install", "php@8.3"]),
            ("sudo", &["apt-get", "install", "-y", "php8.3-cli", "php8.3-mbstring", "php8.3-xml", "php8.3-sqlite3"]),
        ]);
    }

    // Install composer
    if check_exists("composer") {
        println!("  {} Composer already installed", icon_ok().green());
    } else {
        println!("  {} Installing Composer...", icon_play().green());
        if console::is_windows() {
            // Chocolatey ships a `composer` package (pulls in PHP if needed).
            run_install_commands(&[("choco", &["install", "composer", "-y"])]);
        } else if console::is_android() {
            // getcomposer.org's installer targets /usr/local/bin, which Termux
            // does not have; Termux packages composer into $PREFIX/bin.
            termux_install(&["composer"]);
        } else {
            let script = r#"php -r "copy('https://getcomposer.org/installer', 'composer-setup.php');" && php composer-setup.php --install-dir=/usr/local/bin --filename=composer && php -r "unlink('composer-setup.php');" "#;
            let _ = console::shell_exec(script);
        }
    }

    println!(
        "  {} Install tina4php: composer global require tina4stack/tina4php",
        icon_info().blue()
    );
}

fn install_ruby() {
    println!("\n{} Installing Ruby...", icon_play().green());

    if console::is_ish() {
        if !ish_install_runtime("ruby") {
            return;
        }
    } else if check_exists("ruby") {
        let version = get_version("ruby", "--version");
        // Check if it's system Ruby (2.x) vs modern Ruby (3+/4+)
        if version.starts_with("ruby 2") {
            println!(
                "  {} System Ruby {} detected — installing modern Ruby...",
                icon_warn().yellow(),
                version.trim()
            );
            let _ = Command::new("brew")
                .args(["install", "ruby"])
                .status();
        } else {
            println!("  {} Ruby already installed ({})", icon_ok().green(), version.trim());
        }
    } else if console::is_android() {
        termux_install(termux_packages("ruby"));
    } else {
        run_install_commands(&[
            ("choco", &["install", "ruby", "-y"]),
            ("brew", &["install", "ruby"]),
            ("sudo", &["apt-get", "install", "-y", "ruby-full"]),
        ]);
    }

    // Install bundler
    if check_exists("bundle") {
        println!("  {} Bundler already installed", icon_ok().green());
    } else {
        println!("  {} Installing Bundler...", icon_play().green());
        let _ = Command::new("gem")
            .args(["install", "bundler"])
            .status();
    }

    if console::is_android() {
        termux_ruby_build_config();
    }

    // Install tina4ruby
    install_tina4_cli("tina4ruby", "gem", &["install", "tina4ruby"]);
}

fn install_nodejs() {
    println!("\n{} Installing Node.js...", icon_play().green());
    if refused_on_ish("nodejs") {
        return;
    }

    if check_exists("node") {
        println!("  {} Node.js already installed", icon_ok().green());
    } else if console::is_android() {
        termux_install(termux_packages("nodejs"));
    } else {
        run_install_commands(&[
            // Windows, admin-free FIRST: winget installs the Node LTS to the user
            // scope with no Administrator. choco's node package writes machine-wide
            // and needs admin, which is exactly what left a user-level box with no
            // node (and therefore no npm) — the "Cannot install tina4nodejs" report.
            ("winget", &["install", "-e", "--id", "OpenJS.NodeJS.LTS", "--silent", "--accept-package-agreements", "--accept-source-agreements"]),
            // Windows fallback (Chocolatey) — needs admin.
            ("choco", &["install", "nodejs-lts", "-y"]),
            ("brew", &["install", "node@22"]),
            // Linux: use NodeSource
            ("sh", &["-c", "curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt-get install -y nodejs"]),
        ]);
    }

    // npm ships WITH Node. If it is still missing here, either Node did not install
    // (no admin-free path succeeded) or it just landed and this process's PATH is
    // stale (a Windows PATH change needs a fresh shell). Running `npm install -g`
    // now would fail with a cryptic "npm not found", so stop with a real next step.
    if !check_exists("npm") {
        eprintln!(
            "  {} Node.js/npm is not on PATH. If Node just installed, open a NEW terminal and re-run `tina4 install nodejs`. Otherwise install Node LTS from https://nodejs.org (or `winget install OpenJS.NodeJS.LTS`) and re-run.",
            icon_fail().red()
        );
        return;
    }
    println!("  {} npm found", icon_ok().green());

    // Install the Node framework globally. The published npm package is
    // `tina4-nodejs` (the monorepo root) — it provides the `tina4nodejs`
    // binary via its bin field. Installing `tina4nodejs` (no hyphen) 404s:
    // that name is only the internal cli workspace package, never published.
    install_tina4_cli("tina4nodejs", "npm", &["install", "-g", "tina4-nodejs"]);
}

fn install_tina4_js() {
    println!("\n{} Installing tina4-js...", icon_play().green());

    let dest = std::path::Path::new("src/public/js");
    if !dest.exists() {
        std::fs::create_dir_all(dest).unwrap_or_else(|e| {
            eprintln!("  {} Failed to create {}: {}", icon_fail().red(), dest.display(), e);
        });
    }

    let target = dest.join("tina4js.min.js");

    // Try downloading latest from GitHub releases
    let url = "https://raw.githubusercontent.com/tina4stack/tina4-js/master/dist/tina4js.min.js";
    println!("  {} Downloading from {}", icon_play().green(), "tina4stack/tina4-js".cyan());

    // Invoke PowerShell / curl directly (no `cmd /C` wrapper). Wrapping a
    // PowerShell pipeline through cmd.exe lets cmd's parser eat the pipe
    // operator before PowerShell sees it — same root cause as the uv
    // installer bug fixed earlier in this file.
    let download_status = if console::is_windows() {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
                    url,
                    target.display()
                ),
            ])
            .status()
    } else {
        Command::new("sh")
            .args([
                "-c",
                &format!("curl -fsSL '{}' -o '{}'", url, target.display()),
            ])
            .status()
    };

    match download_status {
        Ok(s) if s.success() => {
            println!("  {} tina4js.min.js installed at {}", icon_ok().green(), target.display());
        }
        _ => {
            // Fallback: check if the framework already bundles it
            let framework_paths = [
                "tina4_python/public/js/tina4js.min.js",
                "src/public/js/tina4js.min.js",
                "lib/tina4/public/js/tina4js.min.js",
                "packages/core/public/js/tina4js.min.js",
            ];
            let mut found = false;
            for path in &framework_paths {
                let p = std::path::Path::new(path);
                if p.exists() && std::fs::copy(p, &target).is_ok() {
                    println!("  {} Copied from framework bundle", icon_ok().green());
                    found = true;
                    break;
                }
            }
            if !found {
                eprintln!(
                    "  {} Download failed. tina4js.min.js is bundled with the framework at /js/tina4js.min.js",
                    icon_warn().yellow()
                );
            }
        }
    }

    println!();
    println!("  Usage in your template:");
    println!("    {}", "<script src=\"/js/tina4js.min.js\"></script>".cyan());
    println!();
}

// ── Helpers ──────────────────────────────────────────────────────

/// Make sure the OS package manager exists before we lean on it:
/// Chocolatey on Windows, Homebrew on macOS. Linux relies on the distro's
/// apt/dnf (always present), so this is a no-op there. Returns true if a
/// usable package manager is available afterwards.
///
/// Note: the Chocolatey bootstrap *and* every later `choco install` require
/// an elevated (Administrator) shell. `tina4 setup` relaunches itself
/// elevated on Windows so this runs with the rights it needs.
pub fn ensure_package_manager() -> bool {
    if console::is_windows() {
        if check_exists("choco") {
            println!("  {} Chocolatey already installed", icon_ok().green());
            return true;
        }
        println!("  {} Installing Chocolatey (Windows package manager)...", icon_play().green());
        // Official one-liner from chocolatey.org. TLS 1.2 (3072) is forced so
        // the download works on older Windows boxes.
        let bootstrap = "Set-ExecutionPolicy Bypass -Scope Process -Force; \
            [System.Net.ServicePointManager]::SecurityProtocol = \
            [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; \
            iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))";
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", bootstrap])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        refresh_choco_path_windows();
        if check_exists("choco") {
            println!("  {} Chocolatey installed", icon_ok().green());
            true
        } else {
            eprintln!(
                "  {} Chocolatey install failed — re-run this from a terminal opened \
                 'as Administrator'.",
                icon_fail().red()
            );
            false
        }
    } else if cfg!(target_os = "macos") {
        if check_exists("brew") {
            println!("  {} Homebrew already installed", icon_ok().green());
            return true;
        }
        println!("  {} Installing Homebrew (macOS package manager)...", icon_play().green());
        // NONINTERACTIVE so it never stops to wait for an Enter / sudo prompt.
        let bootstrap = "NONINTERACTIVE=1 /bin/bash -c \"$(curl -fsSL \
            https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";
        let _ = Command::new("sh")
            .args(["-c", bootstrap])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        refresh_brew_path_macos();
        if check_exists("brew") {
            println!("  {} Homebrew installed", icon_ok().green());
            true
        } else {
            eprintln!("  {} Homebrew install failed — see https://brew.sh", icon_fail().red());
            false
        }
    } else {
        // Linux: apt / dnf ship with the distro.
        true
    }
}

/// macOS only: Homebrew, `git`, and the PHP/Ruby/Node runtimes all require the
/// Xcode Command Line Tools. On a fresh Mac they're absent, and the Homebrew
/// bootstrap then fails opaquely — its NONINTERACTIVE mode needs passwordless
/// sudo to install the CLT, which a vanilla machine doesn't have. So detect them
/// up front: if missing, kick off Apple's `xcode-select --install` GUI installer,
/// tell the user what's happening, and return false so the caller stops cleanly
/// (they re-run `tina4 setup` once it finishes). Python never calls this — it's
/// toolchain-free via uv. Returns true when the tools are present, or off macOS.
pub fn ensure_macos_build_tools() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    // `xcode-select -p` prints the active developer directory and exits 0 only
    // when the Command Line Tools (or full Xcode) are installed.
    let present = Command::new("xcode-select")
        .arg("-p")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if present {
        return true;
    }
    println!();
    println!(
        "  {} macOS needs the Xcode Command Line Tools before this step.",
        icon_warn().yellow()
    );
    println!("    Homebrew, git, and the PHP/Ruby/Node runtimes all depend on them.");
    println!(
        "    {} Python needs none of this — re-run setup and pick Python to skip it entirely.",
        "Tip:".bold()
    );
    println!();
    println!(
        "  {} Opening Apple's Command Line Tools installer...",
        icon_play().green()
    );
    let _ = Command::new("xcode-select").arg("--install").status();
    println!();
    println!(
        "  {} A system dialog is installing the tools (a few minutes).",
        icon_info().blue()
    );
    println!("    When it finishes, run {} again.", "tina4 setup".bold());
    println!();
    false
}

/// choco installs its shims to C:\ProgramData\chocolatey\bin. Env-var changes
/// don't reach a running process, so splice that dir into our PATH to find
/// `choco` (and packages it installs) without opening a new shell.
fn refresh_choco_path_windows() {
    let dir = std::env::var("ChocolateyInstall")
        .map(|p| format!("{p}\\bin"))
        .unwrap_or_else(|_| "C:\\ProgramData\\chocolatey\\bin".to_string());
    if !std::path::Path::new(&dir).exists() {
        return;
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current.split(';').map(|s| s.to_string()).collect();
    if !parts.iter().any(|p| p.eq_ignore_ascii_case(&dir)) {
        parts.insert(0, dir);
        std::env::set_var("PATH", parts.join(";"));
    }
}

/// Homebrew lives at /opt/homebrew/bin (Apple Silicon) or /usr/local/bin
/// (Intel). Add whichever exists to PATH for this process.
fn refresh_brew_path_macos() {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current.split(':').map(|s| s.to_string()).collect();
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if std::path::Path::new(dir).exists() && !parts.iter().any(|p| p == dir) {
            parts.insert(0, dir.to_string());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

fn install_codeql() {
    println!("\n{} Installing CodeQL (powers `tina4 codeql`)...", icon_play().green());

    if check_exists("codeql") {
        println!("  {} codeql already installed", icon_ok().green());
    } else if console::is_windows() {
        // CodeQL has no native Windows package that lands `codeql` on PATH, so
        // we guide rather than guess: the gh extension or the official zip.
        println!("  {} CodeQL has no native Windows package.", icon_warn().yellow());
        println!("     Install the GitHub CLI, then: {}", "gh extensions install github/gh-codeql".cyan());
        println!(
            "     or download {} from {} and add it to PATH",
            "codeql-win64.zip".cyan(),
            "https://github.com/github/codeql-cli-binaries/releases".cyan()
        );
        return;
    } else {
        // macOS + Linux: Homebrew ships a codeql formula.
        run_install_commands(&[("brew", &["install", "codeql"])]);
    }

    // The query packs the security-extended suite needs (idempotent re-download).
    if check_exists("codeql") {
        println!("  {} Downloading CodeQL query packs...", icon_play().green());
        let _ = Command::new("codeql")
            .args([
                "pack", "download",
                "codeql/javascript-queries", "codeql/ruby-queries",
                "codeql/python-queries", "codeql/actions-queries",
            ])
            .status();
        println!("  {} Pre-check a repo with: {}", icon_info().blue(), "tina4 codeql".cyan());
    }
}

/// tina4-python needs Python 3.12+. Older Alpine releases ship older Python
/// (iSH's App Store snapshot is Alpine v3.14, which has 3.9), so say so plainly
/// instead of letting `uv tool install` fail with a resolver error later.
/// True (after warning) when the installed Python is too old.
fn python_too_old() -> bool {
    let out = get_version("python3", "--version");
    if let Some((major, minor)) = parse_python_version(&out) {
        if (major, minor) < (3, 12) {
            println!(
                "  {} Python {}.{} is too old - Tina4 needs 3.12+, which Alpine ships from v3.20.",
                icon_warn().yellow(),
                major,
                minor
            );
            return true;
        }
    }
    false
}

/// "Python 3.9.18" -> (3, 9). None when the text is not a Python version line.
fn parse_python_version(text: &str) -> Option<(u32, u32)> {
    let ver = text.trim().strip_prefix("Python ")?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn check_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn get_version(cmd: &str, flag: &str) -> String {
    Command::new(crate::console::resolve_cmd(cmd))
        .arg(flag)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// The Termux (Android) packages that give a language its runtime and package
/// manager. Termux has no sudo and none of apt's distro package names, and the
/// Homebrew / NodeSource / getcomposer.org paths install to locations Termux
/// does not have (/usr/local/bin). Termux's own `pkg` builds every one of these
/// against Bionic and installs into $PREFIX.
pub fn termux_packages(lang: &str) -> &'static [&'static str] {
    match lang {
        "python" | "py" => &["python", "uv"],
        "php" => &["php", "composer"],
        // The sqlite3 gem's library comes from termux_ruby_build_config.
        "ruby" | "rb" => &["ruby"],
        "nodejs" | "node" | "js" | "tina4js" => &["nodejs-lts"],
        _ => &[],
    }
}

/// `pkg install -y` the given Termux packages. True when pkg succeeded.
pub fn termux_install(packages: &[&str]) -> bool {
    println!("  {} Running: pkg install -y {}", icon_play().green(), packages.join(" "));
    match Command::new(crate::console::resolve_cmd("pkg"))
        .args(["install", "-y"])
        .args(packages)
        .status()
    {
        Ok(s) if s.success() => {
            println!("  {} Installed successfully", icon_ok().green());
            true
        }
        _ => {
            eprintln!(
                "  {} pkg install failed. Run it yourself: pkg install {}",
                icon_fail().red(),
                packages.join(" ")
            );
            false
        }
    }
}

/// The sqlite3 gem publishes precompiled builds for Linux (glibc and musl),
/// macOS and Windows, but none for Android, so on Termux bundler compiles it.
/// Its default source build vendors SQLite through mini_portile2, which is not
/// installed, and fails; building against Termux's own libsqlite works. Set
/// globally so a later plain `bundle install` builds it the same way.
pub fn termux_ruby_build_config() {
    // Termux's ruby package does not depend on libsqlite.
    let header = std::env::var("PREFIX")
        .map(|p| std::path::Path::new(&p).join("include/sqlite3.h").exists())
        .unwrap_or(false);
    if !header {
        termux_install(&["libsqlite"]);
    }
    let ok = Command::new(crate::console::resolve_cmd("bundle"))
        .args(["config", "set", "--global", "build.sqlite3", "--enable-system-libraries"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("  {} Bundler set to build sqlite3 against Termux's SQLite", icon_ok().green());
    }
}

/// iSH (iOS): the Alpine packages that give a language a runtime the emulator
/// can run. iSH runs as root with no sudo, and Alpine is its only package
/// source. Python gets no uv: uv dies with SIGILL under iSH as soon as it
/// resolves a package, so its dependencies go through `venv` + pip instead.
/// ruby-bundler brings `bundle`; the scaffold's sqlite3 gem ships a prebuilt
/// x86-linux-musl build, so nothing is compiled.
pub fn ish_packages(lang: &str) -> &'static [&'static str] {
    match lang {
        "python" | "py" => &["python3"],
        "ruby" | "rb" => &["ruby", "ruby-bundler"],
        _ => &[],
    }
}

/// iSH (iOS): why a language cannot run there, or None when it can. Both are
/// the emulator's limits, not Tina4's, so nothing is installed for them.
pub fn ish_unsupported(lang: &str) -> Option<&'static str> {
    match lang {
        "php" => Some(
            "PHP projects need Composer, which cannot run under iSH: the emulator \
             computes PHP's SHA-512 wrongly, so Composer fails its own signature \
             check (and PHP 8.5 crashes on start)",
        ),
        "nodejs" | "node" | "js" | "tina4js" | "tina4-js" | "js-frontend" => Some(
            "Node.js projects need npm, which crashes under iSH (and node itself \
             does not start on the App Store build of iSH)",
        ),
        _ => None,
    }
}

/// `apk add` the given packages. True when apk succeeded.
pub fn apk_install(packages: &[&str]) -> bool {
    println!("  {} Running: apk add {}", icon_play().green(), packages.join(" "));
    match Command::new("apk").arg("add").args(packages).status() {
        Ok(s) if s.success() => {
            println!("  {} Installed successfully", icon_ok().green());
            true
        }
        _ => {
            eprintln!(
                "  {} apk add failed. Run it yourself: apk add {}",
                icon_fail().red(),
                packages.join(" ")
            );
            false
        }
    }
}

/// iSH (iOS): install the language's runtime with apk, then confirm it is new
/// enough. The App Store build of iSH starts on Alpine 3.14, whose Python (3.9)
/// and Ruby (2.7) are both too old for Tina4, so say how to move apk to a
/// current Alpine instead of failing later in the dependency install.
pub fn ish_install_runtime(lang: &str) -> bool {
    println!("  {} iOS (iSH) support is experimental.", icon_warn().yellow());
    let packages = ish_packages(lang);
    if packages.iter().any(|p| !check_exists(ish_command(p))) && !apk_install(packages) {
        return false;
    }
    if ish_runtime_too_old(lang) {
        println!("  {} On iSH, point apk at a current Alpine, then upgrade and re-run:", icon_info().blue());
        println!("      printf '%s\\n' {ALPINE_LATEST}/main {ALPINE_LATEST}/community > /etc/apk/repositories");
        println!("      apk upgrade --available");
        return false;
    }
    true
}

const ALPINE_LATEST: &str = "https://dl-cdn.alpinelinux.org/alpine/latest-stable";

/// The command an Alpine package provides, for the "already installed" check.
fn ish_command(package: &str) -> &str {
    match package {
        "ruby-bundler" => "bundle",
        other => other,
    }
}

fn ish_runtime_too_old(lang: &str) -> bool {
    match lang {
        "python" | "py" => python_too_old(),
        "ruby" | "rb" => ruby_too_old(),
        _ => false,
    }
}

/// tina4ruby needs Ruby 3.1+. True (after warning) when the installed Ruby is older.
fn ruby_too_old() -> bool {
    let out = get_version("ruby", "--version");
    match parse_ruby_version(&out) {
        Some((major, minor)) if (major, minor) < (3, 1) => {
            println!(
                "  {} Ruby {}.{} is too old - Tina4 needs 3.1+.",
                icon_warn().yellow(),
                major,
                minor
            );
            true
        }
        _ => false,
    }
}

/// "ruby 3.4.9 (2026-03-11 ...)" -> (3, 4). None when the text is not a Ruby version line.
fn parse_ruby_version(text: &str) -> Option<(u32, u32)> {
    let ver = text.trim().strip_prefix("ruby ")?;
    let mut parts = ver.split(['.', ' ']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn run_install_commands(attempts: &[(&str, &[&str])]) {
    for (cmd, args) in attempts {
        if check_exists(cmd) {
            println!("  {} Running: {} {}", icon_play().green(), cmd, args.join(" "));
            let status = Command::new(crate::console::resolve_cmd(cmd)).args(*args).status();
            match status {
                Ok(s) if s.success() => {
                    println!("  {} Installed successfully", icon_ok().green());
                    return;
                }
                _ => continue,
            }
        }
    }
    eprintln!(
        "  {} Could not install automatically. Please install manually.",
        icon_fail().red()
    );
}

fn install_tina4_cli(cli_name: &str, pkg_cmd: &str, args: &[&str]) {
    if check_exists(cli_name) {
        println!("  {} {} already installed", icon_ok().green(), cli_name);
    } else if check_exists(pkg_cmd) {
        println!("  {} Installing {}...", icon_play().green(), cli_name);
        match Command::new(crate::console::resolve_cmd(pkg_cmd)).args(args).status() {
            Ok(s) if s.success() => {}
            // Say so: a crashed installer (uv dies with SIGILL under iSH's
            // emulator) otherwise ends the run in silence.
            Ok(s) => eprintln!("  {} {} was not installed ({} exited with {})", icon_fail().red(), cli_name, pkg_cmd, s),
            Err(e) => eprintln!("  {} {} was not installed: {}", icon_fail().red(), cli_name, e),
        }
    } else {
        println!(
            "  {} Cannot install {} — {} not found",
            icon_warn().yellow(),
            cli_name,
            pkg_cmd
        );
    }
}

#[cfg(test)]
mod termux_package_tests {
    #[test]
    fn every_runtime_init_can_ask_for_has_termux_packages() {
        // init's ensure_runtime passes these labels; an empty list would run a
        // bare `pkg install -y` and report success with nothing installed.
        for label in ["python", "php", "ruby", "node"] {
            assert!(!super::termux_packages(label).is_empty(), "{label}");
        }
        // and the names `tina4 install` accepts
        for lang in ["py", "rb", "nodejs", "js", "tina4js"] {
            assert!(!super::termux_packages(lang).is_empty(), "{lang}");
        }
    }

    #[test]
    fn each_runtime_brings_its_package_manager() {
        assert!(super::termux_packages("python").contains(&"uv"));
        assert!(super::termux_packages("php").contains(&"composer"));
        // npm ships inside nodejs-lts, bundler inside ruby
        assert_eq!(super::termux_packages("nodejs"), ["nodejs-lts"]);
        assert!(super::termux_packages("ruby").contains(&"ruby"));
    }
}

#[cfg(test)]
mod python_version_tests {
    #[test]
    fn parses_python_version_lines() {
        assert_eq!(super::parse_python_version("Python 3.9.18\n"), Some((3, 9)));
        assert_eq!(super::parse_python_version("Python 3.12.4"), Some((3, 12)));
        assert_eq!(super::parse_python_version(""), None);
        assert_eq!(super::parse_python_version("command not found"), None);
    }
}

#[cfg(test)]
mod ish_tests {
    #[test]
    fn every_language_init_takes_is_either_installable_or_refused_on_ish() {
        // check_runtime passes these to ish_runtime. An empty package list that
        // is not refused would run a bare `apk add` and report success.
        for lang in ["python", "php", "ruby", "nodejs", "tina4js", "py", "rb", "node", "js"] {
            let refused = super::ish_unsupported(lang).is_some();
            let packages = !super::ish_packages(lang).is_empty();
            assert!(refused != packages, "{lang}: refused={refused} packages={packages}");
        }
    }

    #[test]
    fn python_on_ish_never_installs_uv() {
        assert!(!super::ish_packages("python").contains(&"uv"));
    }

    #[test]
    fn every_ish_package_maps_to_the_command_it_provides() {
        assert_eq!(super::ish_command("python3"), "python3");
        assert_eq!(super::ish_command("ruby"), "ruby");
        assert_eq!(super::ish_command("ruby-bundler"), "bundle");
    }

    #[test]
    fn parses_ruby_version_lines() {
        assert_eq!(super::parse_ruby_version("ruby 3.4.9 (2026-03-11 revision 76cca827ab) +PRISM [i586-linux-musl]\n"), Some((3, 4)));
        assert_eq!(super::parse_ruby_version("ruby 2.7.8p225 (2023-03-30 revision 1f4d455848) [i586-linux-musl]"), Some((2, 7)));
        assert_eq!(super::parse_ruby_version(""), None);
        assert_eq!(super::parse_ruby_version("sh: ruby: not found"), None);
    }
}
