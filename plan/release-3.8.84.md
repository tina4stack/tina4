# Release Tina4 Client 3.8.84

**Outcome:** Publish the signed Tina4 CLI with honest download-failure reporting
and a `tina4 init js` scaffold whose dependencies actually install.

## Scope

- [x] Download failures in `tina4 update`, `tina4 docs`, and `tina4 books` report
  the REAL cause (unreachable host vs a write/disk/permission failure) and exit
  non-zero, instead of always blaming the network and exiting 0 (#21).
- [x] `tina4 init js` pins `vitest ^5`; 4.x could not be installed
  (`npm error Cannot read properties of null (reading 'edgesOut')`), which left a
  freshly scaffolded project with no working dependencies (#20).
- [x] Keep the CLI runtime dependency-free; no framework runtime dependency changes.

## Verification

- [x] `cargo clippy -- -D warnings` clean.
- [x] `cargo test --locked` passed (unit + the new download_failures and scaffold
  integration tests); the binary reports `tina4 3.8.84`.
- [x] `cargo build --release --locked` passed.

## Published

- Client tag: `v3.8.84`.
- Release: <https://github.com/tina4stack/tina4/releases/tag/v3.8.84>
- SimplySign produced the signed Windows artifact; checksums were regenerated over
  the signed bytes before the draft was published.

Status: In Progress
