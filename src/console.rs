//! Cross-platform console helpers — Unicode icons with ASCII fallbacks for old Windows terminals.

/// Enable ANSI escape codes on Windows (virtual terminal processing).
/// Call once at startup. No-op on non-Windows platforms.
pub fn enable_ansi() {
    #[cfg(target_os = "windows")]
    {
        // The `colored` crate calls this internally, but doing it explicitly
        // ensures it runs before any output. Safe to call multiple times.
        let _ = colored::control::set_virtual_terminal(true);
    }
}

/// Returns true if the terminal is likely to render Unicode glyphs correctly.
fn supports_unicode() -> bool {
    if !cfg!(target_os = "windows") {
        return true;
    }
    // Windows Terminal sets WT_SESSION; VS Code sets TERM_PROGRAM
    std::env::var("WT_SESSION").is_ok() || std::env::var("TERM_PROGRAM").is_ok()
}

// ── Icon helpers ──────────────────────────────────────────────
pub fn icon_ok() -> &'static str {
    if supports_unicode() { "✓" } else { "+" }
}

pub fn icon_fail() -> &'static str {
    if supports_unicode() { "✗" } else { "x" }
}

pub fn icon_play() -> &'static str {
    if supports_unicode() { "▶" } else { ">" }
}

pub fn icon_info() -> &'static str {
    if supports_unicode() { "ℹ" } else { "i" }
}

pub fn icon_dash() -> &'static str {
    if supports_unicode() { "—" } else { "-" }
}

pub fn icon_warn() -> &'static str {
    if supports_unicode() { "⚠" } else { "!" }
}

pub fn icon_eye() -> &'static str {
    if supports_unicode() { "👁" } else { "*" }
}

/// Returns true when running on Windows.
pub fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Run a shell command string cross-platform.
/// On Unix uses `sh -c`, on Windows uses `cmd /C`.
pub fn shell_exec(cmd: &str) -> std::io::Result<std::process::ExitStatus> {
    if is_windows() {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    }
}

/// Run a shell command string and capture output (cross-platform).
pub fn shell_output(cmd: &str) -> std::io::Result<std::process::Output> {
    if is_windows() {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .output()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .output()
    }
}

/// Get the correct Python command for the platform.
/// Windows only has `python`, Unix prefers `python3`.
pub fn python_cmd() -> &'static str {
    if is_windows() {
        "python"
    } else if which::which("python3").is_ok() {
        "python3"
    } else {
        "python"
    }
}

/// Find an available port starting from `start`, trying up to `max_tries` ports.
/// Returns the first available port, or the original if all are taken.
pub fn find_available_port(start: u16, max_tries: u16) -> u16 {
    for offset in 0..max_tries {
        let port = start + offset;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

/// Kill whatever process is listening on the given port.
/// Uses `lsof` on macOS/Linux. Returns true if a process was killed.
pub fn kill_port(port: u16) -> bool {
    // Check if port is actually in use
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
        return false; // Port is free, nothing to kill
    }

    #[cfg(unix)]
    {
        // Find PID using lsof
        let output = std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{}", port)])
            .output();

        if let Ok(output) = output {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid_str in pids.trim().lines() {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    // Don't kill our own process
                    let our_pid = std::process::id() as i32;
                    if pid != our_pid {
                        unsafe {
                            libc::kill(pid, libc::SIGTERM);
                        }
                    }
                }
            }
            // Wait briefly for processes to exit
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Verify port is now free
            return std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        }
    }

    #[cfg(windows)]
    {
        // Windows: use netstat + taskkill
        let output = std::process::Command::new("cmd")
            .args(["/C", &format!("for /f \"tokens=5\" %a in ('netstat -aon ^| find \":{} \" ^| find \"LISTENING\"') do taskkill /F /PID %a", port)])
            .output();

        if output.is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(500));
            return std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        }
    }

    false
}

/// Open the default browser to the given URL. Cross-platform.
pub fn open_browser(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        std::process::Command::new("xdg-open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
}

/// Get the PHP vendor binary path.
/// Always returns the PHP script path (not the .bat wrapper),
/// since we invoke it via `php <path>`.
pub fn php_vendor_bin(name: &str) -> String {
    if is_windows() {
        format!("vendor\\bin\\{}", name)
    } else {
        format!("vendor/bin/{}", name)
    }
}
