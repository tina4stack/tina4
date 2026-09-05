# Release Tina4 Client 3.8.83

**Outcome:** Publish the signed Tina4 CLI with reproducible Web Push skills
installation pinned to framework `3.13.134`.

## Scope

- Pin both native skills installers to the released framework tag `3.13.134`.
- Include the Web Push reference in every language developer skill installation.
- Publish the matching checksum manifest and update the public documentation bootstrap.
- Sign and verify `install-skills.ps1` with SimplySign.
- Keep the CLI runtime dependency-free; no framework runtime dependency changes.

## Verification

- Shell installer retry/fallback contract passed against all 43 staged skill files.
- `sh -n install-skills.sh` passed.
- `skills.sha256` regenerated and matches the source skill trees.
- `osslsigncode verify install-skills.ps1` passed with a Code Infinity signature.
- `cargo test --locked` and `cargo build --release --locked` are the release gates.
