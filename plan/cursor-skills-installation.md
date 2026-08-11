# Cursor Skills Installation Plan

## Goal

Add Cursor as a first-class Tina4 AI-skills target, matching the Codex pattern:
canonical content stays in `.claude/skills`; Cursor gets repo-local entrypoints
under `.cursor/skills` and a global install into `~/.cursor/skills`.

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
- [ ] Re-Authenticode-sign `install-skills.ps1` on the next release cut

## Safety

- Never modify project files during a global skill refresh.
- Installer still downloads full skill bodies from `.claude/skills` on the pinned ref.
- Repo-local `.cursor/skills` are entrypoints only — edit the canonical `.claude` copy.
