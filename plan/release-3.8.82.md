# Release Tina4 Client 3.8.82

**Outcome:** Publish signed Tina4 CLI `v3.8.82` with the generic source-root metrics runner and bounded per-file metric history.

## Scope

- [x] Confirm `v3.8.81` is the current published client and `v3.8.82` is unreserved.
- [x] Bump client metadata and record release notes.
- [x] Run the locked local release gate at the release commit.
- [x] Push `main` and tag `v3.8.82` to trigger the draft release.
- [x] Confirm CI audit, all seven binaries, Debian packages, checksums, provenance, and crate publication.
- [x] EV-sign the Windows binary through the authenticated SimplySign session.
- [x] Verify the signed binary and checksums, then publish the draft.
- [x] Verify public assets, crate version, and downstream manifest workflow.

## Release notes

- Metrics accepts any supported source directory or source file; it no longer assumes a Tina4 framework checkout.
- File exclusions are controlled by repeatable CLI switches rather than framework-specific hard-coded paths.
- Duplicate findings include source-file and line-range occurrences in JSON output.
- Metrics history now retains bounded per-file snapshots and changed-file deltas in `.tina4-metrics.json`.
- `--no-history` provides a read-only metrics run, while deleting `.tina4-metrics.json` clears the local history.
- No new runtime dependency was added.

## Commits

- `ffdd7ba` — support arbitrary metrics source roots and clone locations.
- `51c11a6` — retain bounded per-file metric history.
- `ac0a364` — prepare and publish the 3.8.82 client release.

## Verification

- Local: `cargo test --locked` — 200 unit tests passed, integration tests passed, 6 environment-dependent tests ignored.
- Local: `cargo clippy --locked --bin tina4 -- -D warnings` passed.
- Local: `cargo build --release --locked`; binary reports `tina4 3.8.82`.
- CI run `33975563214` passed audit, all seven builds, release assets, and CLI image publication.
- SimplySign/jsign produced a valid Code Infinity EV Authenticode signature with a Certum timestamp.
- Published asset checksums verified; `cargo search tina4` reports `3.8.82`.
- Post-publish manifest workflow `33975832438` passed.

Published: <https://github.com/tina4stack/tina4/releases/tag/v3.8.82>

Status: Complete
