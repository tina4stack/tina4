# Cursor Skills Installation and v3.8.69 Release Plan

## Goal

Add Cursor as a first-class Tina4 AI-skills target, matching the Codex pattern:
canonical content stays in `.claude/skills`; Cursor gets repo-local entrypoints
under `.cursor/skills` and a global install into `~/.cursor/skills`.

Prepare a signed `v3.8.69` client release after the already-published `v3.8.68`.

## Component ownership

| Component | Repository | Responsibility |
|---|---|---|
| Tina4 client | `tina4stack/tina4` | `tina4 skills cursor`, installers, doctor, setup, update refresh |
| Framework repos | python / php / ruby / nodejs | `.cursor/skills/` entrypoints → `.claude/skills/` |
| Frontend | `tina4stack/tina4-js` | `.cursor/skills/tina4-js` entrypoint |
| Documentation | `tina4stack/tina4-documentation` | Hosted installer still delegates to the client |

## Checklist

- [x] Add `.cursor/skills` thin entrypoints in each framework repo (same shape as `.agents/skills`)
- [x] Extend `install-skills.sh` / `.ps1` with `cursor` and include it in `all`
- [x] Wire `tina4 skills cursor` through `setup::install_skills`
- [x] Extend `tina4 doctor` with a Cursor currency section
- [x] Refresh Cursor skills on `tina4 update` when already installed
- [x] Offer Cursor in `tina4 setup` AI choice
- [x] Update README / AGENTS.md discovery notes
- [x] Make setup support multiple AI targets, including `all`
- [x] Add real CLI tests for Cursor selection and update target detection
- [x] Validate all five repository entrypoints and commit any remaining ones
- [x] Bump the client version to `3.8.69`; run the release test suite and build
- [ ] Commit and push the client changes
- [ ] Create and push signed tag `v3.8.69`; wait for the draft-release build
- [ ] Re-Authenticode-sign the Windows `.exe` and `install-skills.ps1`
- [ ] Regenerate `SHA256SUMS`, verify every asset, and publish the release

## Safety

- Never modify project files during a global skill refresh.
- Installer still downloads full skill bodies from `.claude/skills` on the pinned ref.
- Repo-local `.cursor/skills` are entrypoints only — edit the canonical `.claude` copy.

## Verification

- `cargo test`: 286 passed, 2 intentionally ignored live tests.
- `cargo build --release`: passed on Windows.
- PowerShell installer parsed; POSIX installer passed `bash -n`.
- `cargo clippy -- -D warnings`: unavailable because this Rust installation has
  no Clippy component.

## Commits

- `4baea8b` Ruby Cursor entrypoints pushed to `v3`.
- `a74d6c2` Documentation updated and pushed to `main`.
