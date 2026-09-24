# Release 3.8.91 security fixes

- [x] Verify updater bytes and refuse downgrades; Windows hash path passed as environment data
- [x] Fail-closed Unix/Windows installers with verified temporary-file promotion
- [x] Preserve checksum lines of unchanged signed-release assets
- [x] Parsed JSON migration to tina4-nodejs and accurate scaffold imports
- [x] 301 Rust tests passed; 4 existing ignored; CI strict Clippy passed
- [x] Real Unix installation and missing-manifest HTTP404 negative check; original installed bytes preserved
- [x] Actual PowerShell hash command handles apostrophe, semicolon and dollar filename safely
- [x] Five pre-sign integrity tests; shell/PowerShell syntax; 223-crate release inventory
- [x] Re-sign install.ps1 after SimplySign session refreshed; independent Authenticode/CRL verification passed
- [ ] Signed DCO/coauthor commit and combined PR; no remote push until signed final

No new runtime secret generator: backend securely manages development secrets.
Original PR40/41 reviewed; lint39 deferred. Invalid line-based JSON migration replaced.
Version3.8.90 tags remain immutable and GitHub release remains an unpublished superseded draft.

Broader optional all-target Clippy check found four preexisting test-only warnings under Rust1.98;
required CI command cargo clippy -- -D warnings passed unchanged.

## Remaining release steps

- [x] Local release build runs and reports3.8.91
- [x] Private advisory recommendation prepared; no external advisory created
- [x] Active SimplySign slot confirmed; final install.ps1 signed
- [x] Independent Authenticode/CRL verification passed; install.ps1 SHA256317762dff27af572fc831ec278a0187d5b10847fb189d55ae23a9376844dcc7b; skills signature unchanged
- [ ] Commit with DCO and Tina4 coauthor; push one final combined PR and attach it
- [ ] Required CI passes including both PS1 signatures from checkout and GitHub Raw
- [ ] Parent approves final merge/tag timing after all security candidates and lab evidence
- [ ] Create immutablev3.8.91 only after merge; build/verify/sign new release assets
- [ ] Coordinate updated installer URLs/hashes with docs; keep bare3.13.138 andv3.8.90 immutable
- [ ] Publish only when parent clears release hold; keep3.8.90GitHubdraft superseded/unpublished

Final installer signed after session refresh; earlier zero-slot failures are preserved in private verification logs.
