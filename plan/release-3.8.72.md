# Task: Release and sign Tina4 CLI v3.8.72

**Outcome:** Publish a signed Tina4 CLI that streams Bonsai thinking + answer tokens into tina4dev.

## Scope
- [x] Bump CLI version to 3.8.72 (Cargo.toml + CLAUDE.md)
- [x] Run the full Rust test suite at release HEAD
- [ ] Commit streaming work + version bump; tag v3.8.72
- [ ] Wait for CI to produce the complete draft release
- [ ] EV-sign the Windows binary, regenerate checksums, and publish
- [ ] Verify the published release asset and latest-release metadata

## Tests (real, positive + negative)
- [x] `parse_lc_sse_splits_thinking_and_content` (thinking vs content vs dict-trap)
- [x] Full `cargo test --locked` (305 executed passed; 6 environment-dependent ignored)
- [ ] Released binary reports `tina4 3.8.72`

## Parity
| Surface | Status |
|---|---|
| Rust supervisor | Streams `/long_context/stream` |
| Dev-admin SPA | thinking + token events (copied into 4 framework trees) |
| MCP stream endpoint | Live `sha-41bf7cf` |

## Tests (real, positive + negative)
- [ ] `parse_lc_sse_splits_thinking_and_content` (thinking vs content vs dict-trap)
- [ ] Full `cargo test --locked`
- [ ] Released binary reports `tina4 3.8.72`

## Commits
