//! Builds the iSH SIGSYS shim (src/ish_sigsys.c) into a shared object that the
//! binary embeds. Only the i686 musl build runs on iSH, so every other target
//! embeds an empty file and needs no C compiler.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SHIM_TARGET: &str = "i686-unknown-linux-musl";

fn main() {
    println!("cargo:rerun-if-changed=src/ish_sigsys.c");
    println!("cargo:rerun-if-env-changed=CC_i686_unknown_linux_musl");
    println!("cargo:rerun-if-env-changed=CC");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("ish_sigsys.so");
    if env::var("TARGET").as_deref() != Ok(SHIM_TARGET) {
        fs::write(&out, b"").expect("write empty iSH shim");
        return;
    }

    let cc = env::var("CC_i686_unknown_linux_musl")
        .or_else(|_| env::var("CC"))
        .unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .args(["-shared", "-fPIC", "-O2", "-s", "-o"])
        .arg(&out)
        .arg("src/ish_sigsys.c")
        .status()
        .unwrap_or_else(|e| panic!("could not run {cc} to build the iSH SIGSYS shim: {e}"));
    assert!(status.success(), "{cc} failed to build the iSH SIGSYS shim");

    // A host compiler would happily build an x86-64 object; iSH can only load i386.
    let elf = fs::read(&out).expect("read iSH shim");
    let is_i386_elf = elf.len() > 20 && elf[..4] == *b"\x7fELF" && elf[4] == 1 && elf[18] == 3;
    assert!(
        is_i386_elf,
        "{cc} did not produce an i386 ELF for the iSH SIGSYS shim; set CC_i686_unknown_linux_musl"
    );
}
