// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! No spawn may name PowerShell by a bare literal, except the sites still known
//! to do it.
//!
//! On Windows `powershell.exe` lives in `System32\WindowsPowerShell\v1.0\`, not
//! in `System32`, so `CreateProcess` given the bare name can only find it
//! through `PATH`. A `PATH` that has lost that directory fails the spawn with no
//! process and nothing printed. The Windows branch cannot be executed in this
//! suite, so the source is the only witness available -- which is exactly why
//! this is an allowlist over the whole tree rather than a handful of needles
//! over one file:
//!
//! * a denylist of four exact strings missed `"powershell.exe"`, the natural
//!   Windows spelling and the identical bug;
//! * a scan of one file said nothing about the seven sites in the other three;
//! * `include_str!` tied the scan to one filename and embedded it in the binary.
//!
//! Counts are asserted in BOTH directions. Fixing a site fails this test until
//! the entry is removed, which is the point: the list has to stay true.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Sites that still spawn PowerShell by bare name, by file.
///
/// Deliberately left alone: none is on the code path of the reported failure,
/// and none has been reproduced. `install.rs` is the Chocolatey and uv
/// bootstraps, `main.rs` is `tina4 update` and `tina4 docs`, `init.rs` writes a
/// scaffolded script. They carry the same defect and want the same treatment,
/// with evidence of their own.
const STILL_BARE: &[(&str, usize)] = &[
    ("src/init.rs", 1),
    ("src/install.rs", 3),
    ("src/main.rs", 3),
];

/// Every way this crate starts a child process.
const SPAWN_OPENERS: &[&str] = &[
    "Command::new(",
    "run_status(",
    "run_status_reason(",
    "run_status_env_reason(",
    "run_cmd(",
];

fn rust_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found.sort();
    assert!(!found.is_empty(), "found no Rust sources under {}", root.display());
    found
}

fn relative(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Bare-literal spawns per file. The needle stops before the closing quote, so
/// `"powershell"` and `"powershell.exe"` both count.
fn bare_spawns() -> BTreeMap<String, usize> {
    let mut tally = BTreeMap::new();
    for file in rust_sources() {
        let text = std::fs::read_to_string(&file).expect("could not read a source file");
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut count = 0;
        for opener in SPAWN_OPENERS {
            for spacing in ["", " "] {
                let needle = format!("{opener}{spacing}\"powershell");
                count += flat.matches(&needle).count();
            }
        }
        if count > 0 {
            tally.insert(relative(&file), count);
        }
    }
    tally
}

#[test]
fn the_set_of_bare_powershell_spawns_has_not_grown() {
    let found = bare_spawns();
    let expected: BTreeMap<String, usize> = STILL_BARE
        .iter()
        .map(|(f, n)| ((*f).to_string(), *n))
        .collect();
    assert_eq!(
        found, expected,
        "\nthe bare-PowerShell-spawn allowlist is out of date.\n\
         A file that gained a count spawns PowerShell by bare name and will fail \
         on a Windows box whose PATH has lost System32\\WindowsPowerShell\\v1.0 \u{2014} \
         route it through setup::windows_powershell().\n\
         A file that lost one was fixed: delete its entry here.\n"
    );
}

/// The call-site gate. Every unit test in `setup.rs` passes with the resolution
/// reverted, because they all exercise the decision and nothing goes through a
/// spawn.
#[test]
fn the_repaired_sites_still_resolve_before_spawning() {
    let mut calls = BTreeMap::new();
    for file in rust_sources() {
        let text = std::fs::read_to_string(&file).expect("could not read a source file");
        let n = text.matches("windows_powershell()").count()
            - text.matches("fn windows_powershell()").count();
        if n > 0 {
            calls.insert(relative(&file), n);
        }
    }
    // setup.rs: the skills spawn, the UAC relaunch, the Claude Code installer.
    // main.rs: the download fallback (reached when C:\Windows\System32\curl.exe
    // is absent -- exactly the machine this resolution exists for), plus the
    // self-update checksum hash (Get-FileHash) that verifies the downloaded
    // binary before it overwrites the running CLI.
    assert_eq!(
        calls.get("src/setup.rs").copied().unwrap_or(0),
        3,
        "a spawn in setup.rs stopped resolving PowerShell before spawning it"
    );
    assert_eq!(
        calls.get("src/main.rs").copied().unwrap_or(0),
        2,
        "a resolved PowerShell spawn in main.rs (download fallback or update hash) regressed"
    );
}

/// A literal bound to a name and spawned later reads past every needle above.
#[test]
fn no_source_binds_powershell_to_a_name() {
    for file in rust_sources() {
        let text = std::fs::read_to_string(&file).expect("could not read a source file");
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for bind in ["= \"powershell", "=\"powershell"] {
            assert!(
                !flat.contains(bind),
                "{} binds a bare powershell literal to a name, which every spawn scan reads past",
                relative(&file)
            );
        }
    }
}
