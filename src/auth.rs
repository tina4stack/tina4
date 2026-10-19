// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
//! `tina4 login` / `register` / `whoami` / `logout` - GitHub-backed auth against
//! profile.tina4.com, using the browser-handoff + poll flow (like `gh`/`vercel`).
//!
//! The portal owns the GitHub OAuth app and signs the user in server-side, so the
//! CLI never holds a client secret. The flow:
//!   1. POST /api/cli/auth/start {mode} -> a request_id + a verification_uri the
//!      user opens in a browser, plus a user_code to type as a fallback.
//!   2. Open that URL; the user finishes GitHub sign-in on the portal.
//!   3. Poll /api/cli/auth/poll {request_id} until it returns the Tina4 token.
//!   4. Store {token, login, profile_url} under the per-user config dir (0600).
//!
//! `main` is sync and reqwest is async-only here, so this module owns a small
//! tokio runtime and `block_on`s the network parts - the file parts stay sync.
//!
//! NOTE: the two portal endpoints are a DEFINED contract, not yet live. The code
//! is built to that contract; the live handshake cannot be exercised until the
//! portal serves those routes.

use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::console::{icon_fail, icon_info, icon_ok};

const DEFAULT_PROFILE_URL: &str = "https://profile.tina4.com";
const HTTP_TIMEOUT_SECS: u64 = 15;
const DEFAULT_POLL_INTERVAL: u64 = 3;
const DEFAULT_EXPIRES_IN: u64 = 600;
const MAX_WAIT_SECS: u64 = 900; // hard ceiling, whatever the server says

/// Stored, per-machine credentials. Mirrors the portal's poll result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Credentials {
    pub token: String,
    pub login: String,
    pub profile_url: String,
}

/// The `start` response: where to send the user, and how to poll.
#[derive(Debug, Clone, Deserialize)]
struct StartResponse {
    request_id: String,
    verification_uri: String,
    #[serde(default)]
    user_code: String,
    #[serde(default = "default_poll_interval")]
    poll_interval: u64,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_poll_interval() -> u64 { DEFAULT_POLL_INTERVAL }
fn default_expires_in() -> u64 { DEFAULT_EXPIRES_IN }

/// One poll outcome.
enum PollState {
    Pending,
    Done(Credentials),
    Expired,
}

// ---- configuration + storage --------------------------------------------------

/// Portal base URL; `TINA4_PROFILE_URL` overrides it (for staging). Trailing
/// slash trimmed so endpoint joins are clean.
fn base_url() -> String {
    std::env::var("TINA4_PROFILE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PROFILE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Per-user config directory for Tina4. Windows: `%APPDATA%\tina4`. Elsewhere:
/// `$XDG_CONFIG_HOME/tina4`, else `$HOME/.config/tina4`. Mirrors how doctor.rs
/// resolves HOME; no extra crate.
pub(crate) fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.join("tina4"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|h| h.join(".config"))
            })
            .map(|p| p.join("tina4"))
    }
}

fn credentials_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("credentials.json"))
}

/// Load stored credentials, or None if not signed in / unreadable.
pub fn load_credentials() -> Option<Credentials> {
    let path = credentials_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Credentials>(&raw).ok()
}

/// Write credentials to disk, 0600 on unix so a token is not world-readable.
fn save_credentials(creds: &Credentials) -> Result<(), String> {
    let path = credentials_path().ok_or("cannot resolve a config directory (set HOME)")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(creds).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn delete_credentials() -> Result<bool, String> {
    let path = match credentials_path() {
        Some(p) => p,
        None => return Ok(false),
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

/// Open a URL in the user's browser; best-effort (the user_code is the fallback).
/// `TINA4_NO_BROWSER` (the same opt-out `serve` honours) suppresses the launch, so
/// headless/CI runs and the auth tests never pop a window - the printed URL and
/// user_code remain the fallback.
fn open_browser(url: &str) -> bool {
    if std::env::var("TINA4_NO_BROWSER").map(|v| v == "true" || v == "1").unwrap_or(false) {
        return false;
    }
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).status()
    } else if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).stdout(Stdio::null()).stderr(Stdio::null()).status()
    } else {
        std::process::Command::new("xdg-open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).status()
    };
    status.map(|s| s.success()).unwrap_or(false)
}

// ---- network (async) -----------------------------------------------------------

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .expect("reqwest client build failed")
}

async fn start(client: &reqwest::Client, mode: &str) -> Result<StartResponse, String> {
    let url = format!("{}/api/cli/auth/start", base_url());
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "mode": mode }))
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sign-in start failed ({})", resp.status()));
    }
    resp.json::<StartResponse>()
        .await
        .map_err(|e| format!("sign-in start returned unreadable JSON: {e}"))
}

async fn poll(client: &reqwest::Client, request_id: &str) -> Result<PollState, String> {
    let url = format!("{}/api/cli/auth/poll", base_url());
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "request_id": request_id }))
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    match resp.status().as_u16() {
        200 => {
            let creds = resp
                .json::<Credentials>()
                .await
                .map_err(|e| format!("sign-in returned unreadable JSON: {e}"))?;
            Ok(PollState::Done(creds))
        }
        202 => Ok(PollState::Pending),
        410 => Ok(PollState::Expired),
        other => Err(format!("sign-in poll failed ({other})")),
    }
}

/// Drive the whole handshake and return the credentials.
async fn run_flow(mode: &str) -> Result<Credentials, String> {
    let client = http_client();
    let started = start(&client, mode).await?;

    let opened = open_browser(&started.verification_uri);
    if opened {
        println!("{} Opening {} in your browser...", icon_info(), started.verification_uri);
    } else {
        println!("{} Open this URL to continue:\n    {}", icon_info(), started.verification_uri);
    }
    if !started.user_code.is_empty() {
        println!("    (if asked, enter code: {})", started.user_code);
    }
    print!("{} Waiting for you to finish signing in with GitHub...", icon_info());
    let _ = std::io::stdout().flush();

    let interval = started.poll_interval.max(1);
    let deadline = started.expires_in.min(MAX_WAIT_SECS).max(interval);
    let mut waited = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        waited += interval;
        match poll(&client, &started.request_id).await? {
            PollState::Done(creds) => {
                println!();
                return Ok(creds);
            }
            PollState::Expired => {
                println!();
                return Err("the sign-in request expired - run `tina4 login` again".into());
            }
            PollState::Pending => {
                print!(".");
                let _ = std::io::stdout().flush();
            }
        }
        if waited >= deadline {
            println!();
            return Err("timed out waiting for sign-in - run `tina4 login` again".into());
        }
    }
}

/// Build a small runtime and run the async flow to completion.
fn block_on_flow(mode: &str) -> Result<Credentials, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start async runtime: {e}"))?;
    rt.block_on(run_flow(mode))
}

// ---- command entry points (sync) ----------------------------------------------

/// `tina4 login` - authenticate this machine; first login provisions the profile.
pub fn login() {
    run_auth("login");
}

/// `tina4 register` - open the sign-up page, then the same GitHub handshake.
pub fn register() {
    println!("{} Register at {}/register, then finish here.", icon_info(), base_url());
    let _ = open_browser(&format!("{}/register", base_url()));
    run_auth("register");
}

fn run_auth(mode: &str) {
    match block_on_flow(mode) {
        Ok(creds) => {
            if let Err(e) = save_credentials(&creds) {
                eprintln!("{} signed in, but could not save credentials: {e}", icon_fail());
                std::process::exit(1);
            }
            println!("{} Signed in as @{}. Profile: {}", icon_ok(), creds.login, creds.profile_url);
        }
        Err(e) => {
            eprintln!("{} {e}", icon_fail());
            std::process::exit(1);
        }
    }
}

/// `tina4 whoami` - show the signed-in profile. `--json` for tooling/dev-admin.
pub fn whoami(json: bool) {
    match load_credentials() {
        Some(creds) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "authenticated": true,
                        "login": creds.login,
                        "profile_url": creds.profile_url,
                    })
                );
            } else {
                println!("{} Signed in as @{} ({})", icon_ok(), creds.login, creds.profile_url);
            }
        }
        None => {
            if json {
                println!("{}", serde_json::json!({ "authenticated": false }));
            } else {
                println!("{} Not signed in. Run `tina4 login`.", icon_info());
            }
            std::process::exit(1);
        }
    }
}

/// `tina4 logout` - remove the stored credentials.
pub fn logout() {
    match delete_credentials() {
        Ok(true) => println!("{} Signed out.", icon_ok()),
        Ok(false) => println!("{} You were not signed in.", icon_info()),
        Err(e) => {
            eprintln!("{} {e}", icon_fail());
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serializes every test that mutates process-wide env (TINA4_PROFILE_URL, HOME,
    // XDG_CONFIG_HOME, TINA4_NO_BROWSER). Rust runs tests in parallel threads in one
    // process, so without this they would clobber each other's env.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn base_url_defaults_and_trims_override() {
        let _g = ENV_LOCK.lock().unwrap();
        // Pure: no network. Default when unset; override wins and the trailing slash goes.
        std::env::remove_var("TINA4_PROFILE_URL");
        assert_eq!(base_url(), DEFAULT_PROFILE_URL);
        std::env::set_var("TINA4_PROFILE_URL", "https://staging.tina4.com/");
        assert_eq!(base_url(), "https://staging.tina4.com");
        std::env::remove_var("TINA4_PROFILE_URL");
    }

    #[test]
    fn credentials_round_trip() {
        let c = Credentials {
            token: "t4_abc".into(),
            login: "octocat".into(),
            profile_url: "https://profile.tina4.com/u/octocat".into(),
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Credentials = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn start_response_applies_defaults() {
        // poll_interval / expires_in are optional on the wire; defaults fill in.
        let r: StartResponse = serde_json::from_str(
            r#"{"request_id":"r1","verification_uri":"https://profile.tina4.com/cli?code=X"}"#,
        )
        .unwrap();
        assert_eq!(r.request_id, "r1");
        assert_eq!(r.poll_interval, DEFAULT_POLL_INTERVAL);
        assert_eq!(r.expires_in, DEFAULT_EXPIRES_IN);
        assert!(r.user_code.is_empty());
    }

    #[test]
    fn done_payload_parses_as_credentials() {
        // The 200 body of /poll must deserialize straight into Credentials.
        let c: Credentials = serde_json::from_str(
            r#"{"token":"t4_xyz","login":"ada","profile_url":"https://profile.tina4.com/u/ada"}"#,
        )
        .unwrap();
        assert_eq!(c.token, "t4_xyz");
        assert_eq!(c.login, "ada");
    }

    #[test]
    fn config_dir_is_under_xdg_when_set() {
        let _g = ENV_LOCK.lock().unwrap();
        #[cfg(not(windows))]
        {
            std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdgtest");
            let d = config_dir().unwrap();
            assert!(d.ends_with("tina4"));
            assert!(d.starts_with("/tmp/xdgtest"));
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}
