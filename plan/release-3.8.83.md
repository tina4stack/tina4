# Release Tina4 Client 3.8.83

**Outcome:** Publish the signed Tina4 CLI with reproducible Web Push skills
installation pinned to framework `3.13.134`.

## Scope

- [x] Pin both native skills installers to the released framework tag `3.13.134`.
- [x] Include the Web Push reference in every language developer skill installation.
- [x] Publish the matching checksum manifest and update the public documentation bootstrap.
- [x] Sign and verify `install-skills.ps1` with SimplySign.
- [x] Keep the CLI runtime dependency-free; no framework runtime dependency changes.

## Verification

- [x] Shell installer retry/fallback contract passed against all 43 staged skill files.
- [x] `sh -n install-skills.sh` passed.
- [x] `skills.sha256` regenerated and matches the source skill trees.
- [x] `osslsigncode verify install-skills.ps1` passed with a Code Infinity signature.
- [x] `cargo test --locked` (200 unit tests plus integration smoke tests) and
  `cargo build --release --locked` passed; the binary reports `tina4 3.8.83`.

## Published

- Framework tags: `3.13.134` on Python, PHP, Ruby, and Node.js.
- Client tag: `v3.8.83`.
- Release: <https://github.com/tina4stack/tina4/releases/tag/v3.8.83>
- SimplySign produced the signed Windows artifact; checksums were regenerated
  over the signed bytes before the draft was published.

Status: Complete
