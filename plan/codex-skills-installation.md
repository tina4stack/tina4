# Codex Skills Installation Plan

## Goal

Make the `tina4` Rust client install and verify the complete Tina4 skill set for both Claude and Codex, using the correct source components and reproducible release refs.

## Component ownership

| Component | Repository | Responsibility |
|---|---|---|
| Tina4 client | `tina4stack/tina4` | `tina4 ai`, setup flow, global installers, doctor checks |
| Python framework | `tina4stack/tina4-python` | Python developer skill; canonical shared skills |
| PHP framework | `tina4stack/tina4-php` | PHP developer skill |
| Ruby framework | `tina4stack/tina4-ruby` | Ruby developer skill |
| Node.js framework | `tina4stack/tina4-nodejs` | Node.js developer skill |
| Frontend framework | `tina4stack/tina4-js` | Tina4-js library and client-project context |
| Documentation | `tina4stack/tina4-documentation` | Hosted copies of the public installers |

## Verified current state

- The installer pin is `3.13.77`, and that tag exists in all four backend framework repositories.
- All six expected source bundles exist on their current local `v3` heads.
- `install-skills.sh` and `install-skills.ps1` install six skills only into `~/.claude/skills`.
- `tina4 doctor` checks only that Claude directory and marker.
- Codex supports personal skills in `~/.agents/skills` and repository-shared skills in `.agents/skills`.

## Checklist

- [ ] Define one skill manifest: the six expected bundles, source repositories, reference files, and release ref.
- [x] Update the shell installer to require an explicit `claude`, `codex`, or `all` target and install the selected skill directory with an independent version marker.
- [x] Update the PowerShell installer with matching target selection, error handling, and exit codes.
- [x] Add Codex to guided setup, including an explicit Claude/Codex choice and generated `AGENTS.md` guidance.
- [x] Update `tina4 doctor` to report Claude and Codex skill status separately.
- [ ] Add tests for the manifest, both installer targets, marker handling, partial-download failure, and doctor status output.
- [x] Update the public installer copies in `tina4-documentation` to delegate to the canonical client installer.
- [x] Verify release-tag availability for every source repository before publishing an installer ref.
- [ ] Authenticode-sign the published PowerShell installer and release tag before distribution; direct execution is blocked on systems that require signed scripts.
- [x] Build the optimized Rust binary and verify crate packaging on Windows.
- [x] Run `tina4-documentation` `docs:build` with `tina4press` available.
- [x] Bump the client from the already-tagged `3.8.64` to `3.8.65` on `feature/release3.8.65`.
- [x] Commit the client (`458e8e0`) and documentation (`cb27831`) changes on their `feature/release3.8.65` branches.
- [ ] Create a GitHub-verified signed `v3.8.65` tag and publish the release.
- [ ] Regenerate and verify release `SHA256SUMS` after the final Windows binary is Authenticode-signed.

## Safety requirements

- Never modify project files during a global skill refresh.
- Never overwrite an existing project `AGENTS.md` without explicit user intent.
- Download to a temporary directory and replace a target skill only after all of its files are available.
- Preserve the pinned release-ref default; allow an explicit override for testing.

## Open design decision

Codex can use global `~/.agents/skills` and repo-local `.agents/skills`. The installer target must be explicit: `claude`, `codex`, or deliberately `all`; `tina4 ai` should generate only concise, project-specific guidance for the selected tool.
