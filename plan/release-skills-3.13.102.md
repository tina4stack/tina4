# Release Skills 3.13.102 and Tina4 Client 3.8.75

**Outcome:** `tina4 skills <target>` and `tina4 update` install the audited unified-client skills from immutable `3.13.102` framework tags, while client `v3.8.75` ships the corrected tina4-js scaffold.

## Scope

- [x] Confirm all four framework skill commits are synchronized and release-ready.
- [x] Push the framework `v3` branches and create immutable `3.13.102` tags.
- [x] Bump canonical and public skills installers from `3.13.100` to `3.13.102`.
- [x] Re-sign the modified PowerShell installer and verify its CMS certificate structure.
- [x] Bump the Tina4 client to `3.8.75` and document the release.
- [x] Tag and push bare `3.13.102` plus client `v3.8.75`.
- [x] Rebase, verify, and push the public documentation wrappers.
- [x] Verify real installs for Claude, Codex, and Cursor resolve `3.13.102` and contain the unified workflow.
- [x] Complete the signed client release when CI publishes the draft artifacts.

## Tests

- [x] Full Tina4 Rust suite and clippy.
- [x] Shell installer retry/fallback integration test.
- [x] PowerShell installer integration test and Authenticode verification on Windows CI.
- [x] Live installer test for all three skill targets against the published framework tags.
- [x] Framework GitHub tags resolve to the intended skill commits.
- [x] Release assets, checksums, and EV-signed Windows binary verified.

## Bugs

- [x] Installer pin left audited skills stranded on `3.13.100` (`b4daa8f`).
- [x] Current client release still scaffolded the older tina4-js dependency (`7da1887`).

## Commits

- `7093020`, `2463041a`, `8b202bc`, `985884c` - synchronized skills tagged `3.13.102` in all frameworks.
- `7da1887` - Tina4 client scaffolds the current tina4-js release.
- `b4daa8f` - prepare client 3.8.75, installer pin, tests, and macOS installer signer.
- `fdffed9` - Authenticode-sign the canonical PowerShell skills installer.
- `db62918` - publish the immutable skills and client release point.
- `82952d3` - publish the public installer wrappers for skills `3.13.102` after rebase.
- `ba38297` - enforce documentation onboarding parity with framework code.

## Status: Complete

Published client: https://github.com/tina4stack/tina4/releases/tag/v3.8.75
