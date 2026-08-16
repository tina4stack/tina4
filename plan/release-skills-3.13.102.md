# Release Skills 3.13.102 and Tina4 Client 3.8.75

**Outcome:** `tina4 skills <target>` and `tina4 update` install the audited unified-client skills from immutable `3.13.102` framework tags, while client `v3.8.75` ships the corrected tina4-js scaffold.

## Scope

- [ ] Confirm all four framework skill commits are synchronized and release-ready.
- [ ] Push the framework `v3` branches and create immutable `3.13.102` tags.
- [ ] Bump canonical and public skills installers from `3.13.100` to `3.13.102`.
- [ ] Re-sign the modified PowerShell installer and verify its Authenticode status.
- [ ] Bump the Tina4 client to `3.8.75` and document the release.
- [ ] Tag and push bare `3.13.102` plus client `v3.8.75`.
- [ ] Rebase, verify, and push the public documentation wrappers.
- [ ] Verify real installs for Claude, Codex, and Cursor resolve `3.13.102` and contain the unified workflow.
- [ ] Complete the signed client release when CI publishes the draft artifacts.

## Tests

- [ ] Full Tina4 Rust suite and clippy.
- [ ] Shell installer retry/fallback integration test.
- [ ] PowerShell installer integration test and Authenticode verification.
- [ ] Live public installer test for all three skill targets.
- [ ] GitHub tags resolve to the intended commits in all five repositories.
- [ ] Release assets, checksums, and signed Windows binary verified.

## Bugs

- [ ] Installer pin leaves audited skills stranded on `3.13.100`.
- [ ] Current client release still scaffolds the older tina4-js dependency.

## Commits

- Pending.

## Status: In Progress
