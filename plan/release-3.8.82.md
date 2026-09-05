# Release Tina4 Client 3.8.82

**Outcome:** Publish signed Tina4 CLI `v3.8.82` with the generic source-root metrics runner and bounded per-file metric history.

## Scope

- [x] Confirm `v3.8.81` is the current published client and `v3.8.82` is unreserved.
- [x] Bump client metadata and record release notes.
- [ ] Run the locked local release gate at the release commit.
- [ ] Push `main` and tag `v3.8.82` to trigger the draft release.
- [ ] Confirm CI audit, all seven binaries, Debian packages, checksums, provenance, and crate publication.
- [ ] EV-sign the Windows binary through the authenticated SimplySign session.
- [ ] Verify the signed binary and checksums, then publish the draft.
- [ ] Verify public assets, crate version, and downstream manifest workflow.

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
