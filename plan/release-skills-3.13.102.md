# Release Skills 3.13.102 and Tina4 Client 3.8.75

**Outcome:** `tina4 skills <target>` and `tina4 update` install the audited unified-client skills from immutable `3.13.102` framework tags, while client `v3.8.75` ships the corrected tina4-js scaffold.

## Scope

- [x] Confirm all four framework skill commits are synchronized and release-ready.
- [x] Push the framework `v3` branches and create immutable `3.13.102` tags.
- [x] Bump canonical and public skills installers from `3.13.100` to `3.13.102`.
- [x] Re-sign the modified PowerShell installer and verify its CMS certificate structure.
- [x] Bump the Tina4 client to `3.8.75` and document the release.
- [ ] Tag and push bare `3.13.102` plus client `v3.8.75`.
- [ ] Rebase, verify, and push the public documentation wrappers.
- [x] Verify real installs for Claude, Codex, and Cursor resolve `3.13.102` and contain the unified workflow.
- [ ] Complete the signed client release when CI publishes the draft artifacts.

## Tests

- [x] Full Tina4 Rust suite and clippy.
- [x] Shell installer retry/fallback integration test.
- [ ] PowerShell installer integration test and Authenticode verification.
- [x] Live installer test for all three skill targets against the published framework tags.
- [x] Framework GitHub tags resolve to the intended skill commits.
- [ ] Release assets, checksums, and signed Windows binary verified.

## Bugs

- [x] Installer pin left audited skills stranded on `3.13.100` (`b4daa8f`).
- [x] Current client release still scaffolded the older tina4-js dependency (`7da1887`).

## Commits

- `7093020`, `2463041a`, `8b202bc`, `985884c` - synchronized skills tagged `3.13.102` in all frameworks.
- `7da1887` - Tina4 client scaffolds the current tina4-js release.
- `b4daa8f` - prepare client 3.8.75, installer pin, tests, and macOS installer signer.
- `fdffed9` - Authenticode-sign the canonical PowerShell skills installer.
- `137e48e` - prepare public installer wrappers for skills `3.13.102`.

## Status: In Progress
