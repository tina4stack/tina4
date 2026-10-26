// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cross-platform console helpers — Unicode icons with ASCII fallbacks for old Windows terminals.

/// Enable ANSI escape codes on Windows (virtual terminal processing) and switch
/// the console to UTF-8 output. Call once at startup. No-op on non-Windows.
pub fn enable_ansi() {
    #[cfg(target_os = "windows")]
    {
        // The `colored` crate calls this internally, but doing it explicitly
        // ensures it runs before any output. Safe to call multiple times.
        let _ = colored::control::set_virtual_terminal(true);

        // Switch the console to UTF-8 (code page 65001). Rust writes UTF-8 to
        // stdout, but a default US Windows console is cp437/cp850, so any
        // non-ASCII byte (an em dash, ellipsis, or Unicode icon) renders as
        // mojibake — the reported "Text is messed up" garbage. Setting the
        // output code page makes the console interpret our bytes correctly.
        // Raw kernel32 FFI keeps this dependency-free.
        extern "system" {
            fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        }
        const CP_UTF8: u32 = 65001;
        unsafe {
            let _ = SetConsoleOutputCP(CP_UTF8);
        }
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

/// True on Android (Termux). Checked at RUNTIME as well as compile time: the
/// release binary Termux users get is the static musl build, which is compiled
/// for `target_os = "linux"`, so `cfg!` alone would miss it. Android has Bionic,
/// not glibc, so anything that picks a binary or a package manager needs this.
pub fn is_android() -> bool {
    cfg!(target_os = "android")
        || std::env::var_os("TERMUX_VERSION").is_some()
        || std::env::var("PREFIX").is_ok_and(|p| p.contains("/com.termux/"))
}

/// True inside iSH, the Linux shell app for iPhone and iPad. iSH emulates 32-bit
/// x86 Alpine in user mode and reports its own kernel release, "4.20.69-ish".
/// The emulator cannot run every program Alpine ships (uv, PHP's Composer and
/// npm all crash under it), so setup and init take a narrower path there.
pub fn is_ish() -> bool {
    kernel_release().is_some_and(|r| is_ish_release(&r))
}

fn is_ish_release(release: &str) -> bool {
    release.ends_with("-ish")
}

#[cfg(unix)]
fn kernel_release() -> Option<String> {
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } != 0 {
        return None;
    }
    let release = unsafe { std::ffi::CStr::from_ptr(uts.release.as_ptr()) };
    Some(release.to_string_lossy().into_owned())
}

#[cfg(not(unix))]
fn kernel_release() -> Option<String> {
    None
}

/// `std::env::current_exe()`, corrected for Termux. termux-exec launches
/// binaries through the system linker (`/apex/.../linker64 <binary>`), so
/// /proc/self/exe names the linker, and self-update or uninstall would act on
/// it. termux-exec puts the real path in TERMUX_EXEC__PROC_SELF_EXE.
pub fn current_exe() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(termux_real_exe(&exe, std::env::var_os("TERMUX_EXEC__PROC_SELF_EXE")).unwrap_or(exe))
}

/// The real binary when `exe` is Android's linker and termux-exec named one.
fn termux_real_exe(
    exe: &std::path::Path,
    proc_self_exe: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let is_linker = exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "linker" || n == "linker64");
    if !is_linker {
        return None;
    }
    proc_self_exe.map(std::path::PathBuf::from).filter(|p| p.is_absolute())
}

#[cfg(test)]
mod current_exe_tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn termux_linker_exec_resolves_to_the_real_binary() {
        let real = "/data/data/com.termux/files/usr/bin/tina4";
        assert_eq!(
            super::termux_real_exe(Path::new("/apex/com.android.runtime/bin/linker64"), Some(real.into())),
            Some(PathBuf::from(real))
        );
    }

    #[test]
    fn anything_else_keeps_its_own_path() {
        // Not the linker: a stray env var must not redirect the update.
        let real = "/data/data/com.termux/files/usr/bin/tina4";
        assert_eq!(super::termux_real_exe(Path::new("/usr/local/bin/tina4"), Some(real.into())), None);
        // The linker, but termux-exec named nothing usable.
        assert_eq!(super::termux_real_exe(Path::new("/system/bin/linker64"), None), None);
        assert_eq!(super::termux_real_exe(Path::new("/system/bin/linker64"), Some("tina4".into())), None);
    }
}

/// The command-line HTTP client to shell out to on Unix. curl when it is
/// installed, otherwise wget: Alpine and iSH ship only busybox wget, Termux
/// only curl, and most desktops have both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Downloader {
    Curl,
    Wget,
}

/// What to tell someone with neither downloader installed.
pub const NO_DOWNLOADER: &str =
    "neither curl nor wget is installed -- install one (e.g. `apk add curl`, `pkg install curl`, `apt install curl`)";

pub fn unix_downloader() -> Option<Downloader> {
    if which::which("curl").is_ok() {
        Some(Downloader::Curl)
    } else if which::which("wget").is_ok() {
        Some(Downloader::Wget)
    } else {
        None
    }
}

impl Downloader {
    pub fn program(self) -> &'static str {
        match self {
            Downloader::Curl => "curl",
            Downloader::Wget => "wget",
        }
    }

    /// Arguments that print `url` to stdout, failing on an HTTP error.
    /// `headers` are `Name: value` lines. busybox wget has no -L: it always
    /// follows redirects, like GNU wget.
    pub fn stdout_args(self, url: &str, headers: &[&str]) -> Vec<String> {
        let (mut args, flag) = match self {
            Downloader::Curl => (vec!["-fsSL".to_string()], "-H"),
            Downloader::Wget => (vec!["-qO-".to_string()], "--header"),
        };
        for h in headers {
            args.push(flag.to_string());
            args.push(h.to_string());
        }
        args.push(url.to_string());
        args
    }

    /// Arguments that save `url` to `dest`, failing on an HTTP error.
    pub fn file_args(self, url: &str, dest: &str) -> Vec<String> {
        match self {
            Downloader::Curl => vec!["-fsSL", "-o", dest, url],
            Downloader::Wget => vec!["-q", "-O", dest, url],
        }
        .into_iter()
        .map(String::from)
        .collect()
    }

    /// A shell command that prints `url` to stdout, for `... | sh` pipelines.
    pub fn stdout_command(self, url: &str) -> String {
        match self {
            Downloader::Curl => format!("curl -LsSf {}", url),
            Downloader::Wget => format!("wget -qO- {}", url),
        }
    }
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

/// What `take_over_port` found on a busy port, and what it did about it.
#[derive(Debug, PartialEq, Eq)]
pub enum Takeover {
    /// The holder was this project's own Tina4 dev server; it was stopped and
    /// the port is free again.
    Reclaimed(Vec<u32>),
    /// The port is held by something that is not this project's Tina4 dev
    /// server. Nothing was signalled.
    Foreign(Vec<u32>),
    /// Takeover was opted out (TINA4_NO_TAKEOVER / `tina4 serve --no-kill`); the
    /// holder was left running, Tina4 or not.
    RefusedOptout,
    /// Not dev mode (a production bind); takeover is disabled and the holder was
    /// left running.
    RefusedProduction,
    /// The port is busy but its holder could not be identified or stopped.
    Failed,
}

/// Truthy in the Tina4 sense, matching every framework's `is_truthy`:
/// `true` / `1` / `yes` / `on`, case-insensitive. Anything else is false.
fn env_is_truthy(value: Option<String>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("true") | Some("1") | Some("yes") | Some("on")
    )
}

/// Takeover is opted out when `TINA4_NO_TAKEOVER` is truthy. `tina4 serve
/// --no-kill` sets this for the caller; the env var lets anything else opt out
/// too (TAKEOVER-DEC-03).
pub fn takeover_opted_out() -> bool {
    env_is_truthy(std::env::var("TINA4_NO_TAKEOVER").ok())
}

/// Whether `tina4 serve` is a dev run for the takeover gate. `serve` is a dev
/// command, so it is dev unless `TINA4_DEBUG` is explicitly falsy (the caller
/// also gates on `--production`). The runtime bind-failure paths resolve dev
/// from `TINA4_DEBUG` alone; both feed the ONE gate, which is the point of
/// TAKEOVER-DEC-03.
pub fn serve_is_dev() -> bool {
    match std::env::var("TINA4_DEBUG") {
        Ok(value) => env_is_truthy(Some(value)),
        Err(_) => true, // unset: a bare `tina4 serve` is a dev run
    }
}

/// The per-port PID file a Tina4 dev server writes when it binds.
///
/// The same path every framework uses — `tina4_python/core/port_takeover.py`,
/// `Tina4/PortTakeover.php`, `lib/tina4/cli.rb`, `packages/core/src/server.ts`
/// — so a server started by any of them is recognised here.
pub fn serve_pidfile(project_dir: &std::path::Path, port: u16) -> std::path::PathBuf {
    project_dir.join("data").join(format!(".tina4-serve-{}.pid", port))
}

/// The PID recorded in a serve PID file, or None when absent or not a number.
fn read_serve_pidfile(path: &std::path::Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// The holders that may be signalled: those whose PID is the one the dev server
/// recorded. Everything else on the port is somebody else's.
///
/// PID 0 and 1 are never signalled whatever the file says — `kill(0, ..)`
/// signals our own process group, and a garbage file must not be able to make
/// `tina4 serve` stop itself — and neither is this process.
fn tina4_holders(holders: &[u32], recorded: Option<u32>, me: u32) -> Vec<u32> {
    match recorded {
        Some(pid) if pid > 1 && pid != me => {
            holders.iter().copied().filter(|h| *h == pid).collect()
        }
        _ => Vec::new(),
    }
}

/// PIDs listening on `port`, as the platform reports them.
fn port_listeners(port: u16) -> Vec<u32> {
    #[cfg(unix)]
    {
        // LISTEN only: without it lsof also names every client connected to the
        // port, and those were being killed along with the server.
        std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{}", port), "-sTCP:LISTEN"])
            .output()
            .map(|o| parse_pids(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("netstat")
            .arg("-ano")
            .output()
            .map(|o| parse_netstat_listeners(&String::from_utf8_lossy(&o.stdout), port))
            .unwrap_or_default()
    }
}

/// Numeric tokens only, de-duplicated: a stray word never becomes PID 0.
fn parse_pids(text: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for pid in text.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
        if !pids.contains(&pid) {
            pids.push(pid);
        }
    }
    pids
}

/// The owning PIDs of `LISTENING` rows in `netstat -ano` output whose local
/// address ends in `:<port>`.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_netstat_listeners(text: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{}", port);
    let mut pids = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Proto, Local Address, Foreign Address, State, PID
        if cols.len() == 5 && cols[0] == "TCP" && cols[1].ends_with(&suffix) && cols[3] == "LISTENING" {
            if let Ok(pid) = cols[4].parse::<u32>() {
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

fn stop_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
}

/// Reclaim a busy `port` only from this project's own Tina4 dev server.
///
/// This used to signal every PID on the port, with no check of what it was: an
/// unrelated server, a database, a browser tab connected to it. The framework
/// ports settled this already (TAKEOVER-DEC-01): a dev server writes
/// `data/.tina4-serve-<port>.pid` when it binds, and only the process named
/// there is taken over. Anything else is refused and left running — the worst
/// case is that the developer frees the port by hand.
pub fn take_over_port(
    port: u16,
    project_dir: &std::path::Path,
    dev: bool,
    no_takeover: bool,
) -> Takeover {
    // TAKEOVER-DEC-03: the opt-out and the dev gate come FIRST, before the port
    // is even inspected, so a held port is never reclaimed in production or when
    // the developer opted out. The holder is left running in both cases. `dev`
    // and `no_takeover` are passed in (resolved by the caller from
    // `serve_is_dev()` / `takeover_opted_out()` + `--no-kill` / `--production`)
    // so this stays the one shared, directly testable gate.
    if no_takeover {
        return Takeover::RefusedOptout;
    }
    if !dev {
        return Takeover::RefusedProduction;
    }
    let holders = port_listeners(port);
    if holders.is_empty() {
        return Takeover::Failed;
    }
    let pidfile = serve_pidfile(project_dir, port);
    let ours = tina4_holders(&holders, read_serve_pidfile(&pidfile), std::process::id());
    if ours.is_empty() {
        return Takeover::Foreign(holders);
    }
    for pid in &ours {
        stop_pid(*pid);
    }
    let _ = std::fs::remove_file(&pidfile);
    std::thread::sleep(std::time::Duration::from_millis(500));
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
        Takeover::Reclaimed(ours)
    } else {
        Takeover::Failed
    }
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

/// Resolve a command name to its full path.
/// On Windows this is critical: `which` finds `composer.bat` but
/// `Command::new("composer")` does NOT — it only searches for `.exe`.
/// By resolving the full path first, `.bat` and `.cmd` wrappers work correctly.
pub fn resolve_cmd(cmd: &str) -> String {
    which::which(cmd)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| cmd.to_string())
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

#[cfg(test)]
mod tests {
    #[test]
    fn ish_is_recognised_by_its_kernel_release() {
        assert!(super::is_ish_release("4.20.69-ish"));
        assert!(!super::is_ish_release("6.8.0-45-generic"));
        assert!(!super::is_ish_release("5.15.0-android13"));
        assert!(!super::is_ish_release(""));
    }

    use super::*;

    #[test]
    fn resolve_cmd_returns_an_absolute_path_for_a_command_on_path() {
        // The interpreter resolution for `tina4 serve` (php/ruby/node) goes
        // through resolve_cmd so Windows and git worktrees find the binary
        // (#120). A command that is always present must resolve to an absolute
        // path, never the bare name — that is the difference from the old
        // Command::new("php") that failed off a login PATH.
        let probe = if is_windows() { "cmd" } else { "sh" };
        if which::which(probe).is_ok() {
            let resolved = resolve_cmd(probe);
            assert!(
                std::path::Path::new(&resolved).is_absolute(),
                "resolve_cmd({probe}) should be absolute, got {resolved}"
            );
            assert_ne!(resolved, probe, "resolve_cmd must not hand back the bare name when found");
        }
    }

    #[test]
    fn resolve_cmd_falls_back_to_the_bare_name_when_absent() {
        // Fallback keeps POSIX behaviour unchanged when the command is not on
        // PATH: the caller still gets a usable value rather than an error.
        assert_eq!(resolve_cmd("tina4-definitely-not-a-real-binary-xyz"), "tina4-definitely-not-a-real-binary-xyz");
    }

    #[test]
    fn only_the_recorded_pid_is_ours() {
        assert_eq!(tina4_holders(&[40, 41], Some(41), 7), vec![41]);
    }

    #[test]
    fn a_holder_with_no_record_is_foreign() {
        assert!(tina4_holders(&[40], None, 7).is_empty());
    }

    #[test]
    fn a_record_naming_another_pid_makes_the_holder_foreign() {
        assert!(tina4_holders(&[40], Some(999_999), 7).is_empty());
    }

    #[test]
    fn a_garbage_record_never_names_our_group_init_or_ourselves() {
        assert!(tina4_holders(&[0, 1, 7], Some(0), 7).is_empty());
        assert!(tina4_holders(&[0, 1, 7], Some(1), 7).is_empty());
        assert!(tina4_holders(&[0, 1, 7], Some(7), 7).is_empty());
    }

    #[test]
    fn pid_tokens_are_numbers_only() {
        assert_eq!(parse_pids("123\nabc\n123\n 456 \n"), vec![123, 456]);
        assert!(parse_pids("lsof: WARNING").is_empty());
    }

    /// `netstat -ano` as Windows prints it: IPv4 and IPv6 rows, clients, UDP.
    #[test]
    fn netstat_rows_yield_listeners_on_the_port_only() {
        let out = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  \
                   TCP    0.0.0.0:7146           0.0.0.0:0              LISTENING       4120\r\n  \
                   TCP    127.0.0.1:7146         127.0.0.1:51000        ESTABLISHED     4120\r\n  \
                   TCP    127.0.0.1:51000        127.0.0.1:7146         ESTABLISHED     9001\r\n  \
                   TCP    0.0.0.0:71460          0.0.0.0:0              LISTENING       5000\r\n  \
                   TCP    [::]:7146              [::]:0                 LISTENING       4120\r\n  \
                   UDP    0.0.0.0:7146           *:*                                    6000\r\n";
        assert_eq!(parse_netstat_listeners(out, 7146), vec![4120]);
    }

    #[test]
    fn the_pidfile_is_where_the_frameworks_write_it() {
        assert_eq!(
            serve_pidfile(std::path::Path::new("/p"), 7146),
            std::path::Path::new("/p/data/.tina4-serve-7146.pid")
        );
    }

    // TAKEOVER-DEC-03: the opt-out and the dev gate are checked before the port
    // is ever inspected, so they hold whatever the port is doing.
    #[test]
    fn opt_out_refuses_before_the_port_is_touched() {
        assert_eq!(
            take_over_port(0, std::path::Path::new("/does-not-exist"), true, true),
            Takeover::RefusedOptout
        );
    }

    #[test]
    fn production_refuses_before_the_port_is_touched() {
        assert_eq!(
            take_over_port(0, std::path::Path::new("/does-not-exist"), false, false),
            Takeover::RefusedProduction
        );
    }

    #[test]
    fn env_truthiness_matches_the_frameworks() {
        for yes in ["true", "TRUE", "1", "yes", "On", " on "] {
            assert!(env_is_truthy(Some(yes.to_string())), "{yes} should be truthy");
        }
        for no in ["false", "0", "no", "off", "", "maybe"] {
            assert!(!env_is_truthy(Some(no.to_string())), "{no} should be falsy");
        }
        assert!(!env_is_truthy(None));
    }
}
