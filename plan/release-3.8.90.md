# Release CLI 3.8.90 and skills 3.13.138

Outcome: release all merged CLI fixes with the latest measured-time skills.

- [x] Prepare isolated worktree from current origin/main.
- [x] Bump Cargo manifest/lock and document all unpublished changes.
- [x] Pin canonical skills installers and regenerate 48 checksums.
- [x] Sign final PowerShell installer and independently verify signature (osslsigncode digest, timestamp, chain, CRL all valid).
- [x] Rust tests296passed/4existingignored, strict Clippy and locked release build pass; binary3.8.90. Local rust-objcopy debug-strip warning did not prevent a working release binary.
- [x] Shell and native PowerShell installer contracts each pass all five retry/fallback/outage/gap/revival modes.
- [x] Commit signed release preparation locally; coordinate one final PR/CI run with root (no remote push yet).
- [ ] Tag/publish only after coordinated release verification.

Root owns documentation wrappers and the immutable skills bundle. No tags or publication in preparation.

## Release-integrity controls
- [x] Own standard-library generator inventories223locked packages; validated against the official SPDX2.3 JSON schema using an already-installed validator.
- [x] Preserve actual upstream licence declarations and notices, with exact source-commit fallback texts for10crates. No approved inbound-policy/legal-conformance claim. Missing or unrecognized identifiers/notices fail PR CI.
- [x] Include notices/inventory beside binaries and inside Debian packages/container. Pin container base digest and explicitly emit max build provenance.
- [x] Attest all draft assets/checksums, including unsigned Windows; all three signing scripts fail closed on mismatched/incomplete checksums or missing tag-bound CI provenance.
- [x] Five real-file checksum rejection tests pass; shell/PowerShell syntax and workflow YAML validated.
- [ ] End-to-end hosted draft attestations and Debian/container packaging require the coordinated release workflow.
- [ ] Inbound licence-policy approval remains a separate legal/maintainer decision; inventory records not-assessed.
