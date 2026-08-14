# Task: Release and sign Tina4 CLI v3.8.70

**Outcome:** Publish a signed Tina4 CLI containing the FREE-TOKEN fallback so dev-admin works without a personal MCP token.

## Scope
- [x] Confirm FREE-TOKEN is active server-side
- [x] Bump CLI version to 3.8.70
- [x] Run the full Rust test suite at release HEAD
- [x] Commit and tag v3.8.70
- [x] Wait for CI to produce the complete draft release
- [x] EV-sign the Windows binary, regenerate checksums, and publish
- [x] Verify the published release asset and latest-release metadata

## Parity
| Surface | Status |
|---|---|
| Rust supervisor | Pending release |
| Dev-admin SPA | Already deployed |
| Hosted FREE-TOKEN | Live, HTTP 200 verified |

## Tests (real, positive + negative)
- [x] Full `cargo test --locked` (303 executed passed; 6 environment-dependent ignored)
- [x] Released binary reports `tina4 3.8.70`
- [x] Published FREE-TOKEN `long_context` call returns successfully

## Bugs
- [x] Dev-admin deployed before the CLI release containing its FREE-TOKEN dependency

## Commits
- 7eb23f3  release: prepare Tina4 CLI 3.8.70
- 0181c89  docs(plan): record 3.8.70 release preparation

## Status: Complete
