# Release Tina4 Client 3.8.76

**Outcome:** Publish signed client `v3.8.76` from production `main` with the corrected native metrics contract, verified binaries, checksums, crate, and multi-architecture container image.

## Scope

- [x] Sweep open Tina4 organization issues for release blockers.
- [x] Confirm `v3.8.75` is current and `v3.8.76` is unreserved.
- [x] Confirm the live SimplySign token, certificate alias, timestamp service, and signing tools.
- [x] Bump client metadata and add release notes.
- [x] Run the full local release gate at the exact release commit.
- [ ] Merge the release branch into `main`, tag `v3.8.76`, and push.
- [ ] Wait for the complete CI draft and supply-chain gate.
- [ ] EV-sign the Windows binary, regenerate checksums, and publish.
- [ ] Verify every public asset, Authenticode signature, checksum, crate, and container image.

## Parity

| Artifact | amd64 | arm64 | Signed/attested |
| --- | --- | --- | --- |
| Linux glibc | ❌ BUILD | ❌ BUILD | ❌ BUILD |
| Linux musl | ❌ BUILD | ❌ BUILD | ❌ BUILD |
| macOS | ❌ BUILD | ❌ BUILD | ❌ BUILD |
| Windows | ❌ BUILD | n/a | ❌ BUILD |
| GHCR image | ❌ BUILD | ❌ BUILD | ❌ BUILD |

## Tests

- [x] `cargo test` passes at release HEAD: 318 passed, 6 environment-dependent ignored.
- [x] `cargo clippy --bin tina4 -- -D warnings` passes.
- [x] `cargo build --release --locked` and `tina4 --version` pass.
- [ ] CI cargo-deny, seven binary builds, smoke tests, crate publish, and image publish pass.
- [ ] Published `SHA256SUMS` verifies all seven binaries.
- [ ] Windows binary verifies as signed by Code Infinity with a valid timestamp.

## Bugs

- [x] No open organization issue blocks the native metrics/signing release.
- [x] CLI-BROKEN-PIPE: restored normal Unix `SIGPIPE` termination; the regression failed before the fix and passes after it.
- [ ] Any release-pipeline failure found during the cut is fixed before publication.

## Commits

- `0e01f13` — bump 3.8.76, add release notes, and fix Unix broken-pipe behavior with a mutation-proven regression.

## Status: In Progress

## Release notes

### Tina4 Client 3.8.76

- Metrics measures production source by default and supports repeatable
  `--exclude` plus `--include-non-production`.
- JSON now reports `has_referencing_test`. It does not claim test execution or
  coverage. The old `has_tests` field is removed before the 3.14.0 stability boundary.
- Parsed test imports, exported symbols, and Ruby namespace handling remove the
  known false positives and false negatives.
- Nested callable scopes now match across Python, PHP, Ruby,
  TypeScript/JavaScript, and Rust.
- Type-2 duplicate detection ignores comments while retaining executable Python docstrings.
- Unix pipelines close cleanly without a Rust Broken-pipe panic.

No new runtime dependency was added.
