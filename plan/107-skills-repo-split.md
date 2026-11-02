# Plan: #107 move the skills bundle to a dedicated `tina4-skills` repo

- Status: Draft for maintainer review (PLAN ONLY - nothing created, moved, or re-pointed yet)
- Repo: `tina4stack/tina4` (the Rust CLI), branch `plan/107-skills-repo-split`
- Issue: tina4#107. Durable root-cause fix for tina4#141 (closed as symptom-cleared by v3.8.113).
- Folds in: tina4#109, tina4#110 (skills-regression guard), tina4#112 (strict-YAML frontmatter).
- Companion ADR stub: `plan/ADR-0102-skills-repo-split.stub.md` (to be filed in tina4-documentation).

## Outcome

No bare `3.x` tag ever lands on the `tina4` (CLI) repo again. The CLI repo's
`releases.atom` then only ever carries `v`-prefixed CLI releases, so the pre-3.8.106
`tina4 update` resolver can never again read a skills tag as the latest CLI release.
The skills bundle gets its own repo, its own tag line, and its own cadence.

## Headline: this is the durable fix for #141, not tidiness

The bug in #141 is a tag-ordering accident, and the accident has a single cause: the
skills release process pushes a **bare `3.13.x` git tag onto the `tina4` CLI repo**.

Why the tag exists at all - trace one skills install. The installer reads the file set
from the `skills.sha256` manifest, and that manifest lives in the `tina4` repo. The
primary tier fetches it flat from tina4.com, but the two CDN fallbacks fetch it from a
tag on the CLI repo:

```
# src/skills.rs  Config::manifest_urls()        tina4/install-skills.sh  verify_checksums()
tina4.com/skills/<ref>/skills.sha256            # flat, repo-independent, no tag
cdn.jsdelivr.net/gh/tina4stack/tina4@<ref>/skills.sha256   # <- needs a tag on tina4
raw.githubusercontent.com/tina4stack/tina4/<ref>/skills.sha256   # <- needs a tag on tina4
```

The `tina4-cli` skill itself is the same story: `repo_for_skill("tina4-cli")` returns
`tina4` (`src/skills.rs:85`), so its CDN fallbacks also resolve against a tag on the CLI
repo. To keep both working, `scripts/bump-skills-ref.sh` tells the releaser to "create
the bare `$version` tag in tina4 and all framework repos". That bare `3.13.x` tag is the
one that tops the CLI repo's `releases.atom`.

What that tag does to an old client, traced through the resolver:

- pre-3.8.106 `tina4 update` reads the first `/releases/tag/` link in
  `tina4/releases.atom` and treats it as "latest";
- the newest entry is the skills tag `3.13.156`, not the newest CLI release `v3.8.112`;
- the client then asks for `v3.13.156` binary assets, which do not exist, and every
  asset 404s - exactly the report in #141.

v3.8.113 put a `v`-tag back on top, so the ordering is correct again today. It changed no
resolver code (CHANGELOG: "No CLI behaviour change"). The moment the next skills bundle
(`3.13.157`) cuts its bare tag, that tag tops the feed again and re-breaks the shrinking
3.8.104 cohort. The ordering fix is a holding action; the tag itself is the defect.

**Move the manifest and the `tina4-cli` skill source to `tina4-skills`, and point the CDN
fallbacks there. Then the bare `3.x` tag lands on `tina4-skills`, the CLI repo carries
only `v`-prefixed tags, and `releases.atom` on the CLI repo can never again name a skills
version.** That is the root-cause fix. Everything else in #107 (decoupled cadence, one
visible home for language-agnostic skills) rides along, but the #141 cure is the reason to
do it now.

## What a skills install touches today (ground truth)

The CLI binary embeds none of this - it fetches every file at runtime (confirmed: the only
`include_str!` in `src/skills.rs` is the file reading itself for a test). So the move is a
change of URLs and source locations, not of shipped binary bytes. Three runtime URLs decide
everything:

| What | Where it is read from | Needs a tag on the CLI repo? |
|------|----------------------|------------------------------|
| latest ref | `https://tina4.com/install-skills.sh` (`SKILLS_INSTALL_URL`) | No - tina4.com, repo-independent |
| `skills.sha256` primary | `tina4.com/skills/<ref>/skills.sha256` | No - flat, Jenkins-served |
| `skills.sha256` CDN fallback | `tina4@<ref>` / `tina4/<ref>` (jsDelivr, raw) | **Yes - this forces the bare tag** |
| `tina4-cli` skill CDN fallback | `repo_for_skill("tina4-cli") == "tina4"` | **Yes - this forces the bare tag** |
| framework dev skills | their own framework repos at `<ref>` | framework release tag (unchanged) |
| shared skills (js/maintainer/architect/design/seo/a11y) | `tina4-python` at `<ref>` | framework release tag (unchanged) |

Only the CLI repo's bare tag is the problem. The framework repos already carry bare
`3.13.x` tags as their own release tags, so no new defect lives there.

## Exactly what moves, with paths

### Moves from `tina4` to `tina4-skills`

| File (in `tina4` today) | Role | Note |
|-------------------------|------|------|
| `install-skills.sh` | POSIX skills installer (standalone, served) | keeps its own hardcoded file list |
| `install-skills.ps1` | PowerShell skills installer (EV-signed) | `.gitattributes` marks it `binary` - carry that rule across |
| `skills.sha256` | checksum manifest (the file-set contract) | the manifest the bare tag was serving |
| `scripts/skills-list.sh` | single source of truth for the file set | sourced by the two generators |
| `scripts/gen-skills-sha256.sh` | emits `skills.sha256` | |
| `scripts/gen-skills-bundle.sh` | stages the tina4.com bundle | |
| `scripts/bump-skills-ref.sh` | bumps the `TINA4_SKILLS_REF` pin | update its instructions (see cadence) |
| `scripts/check-skills-regression.sh` | regression guard (#106, extended by #109) | |
| `scripts/validate-skills-frontmatter.py` | strict-YAML frontmatter gate (#112) | |
| `scripts/sign-skills-installer-mac.sh` | jsign + SimplySign signer for `install-skills.ps1` | skills-only signer, clean move |
| `scripts/sign-installers-mac.sh` | osslsigncode signer, defaults to `install-skills.ps1` | signs only the skills installer by default |
| `scripts/sign-installers.ps1` | signtool signer, defaults to `install-skills.ps1` | signs only the skills installer by default |
| `.claude/skills/tina4-cli/` | the `tina4-cli` skill source (`SKILL.md` + `references/commands.md`) | see the open question on this one |
| `tests/skills_installer_http.py` | installer retry/fallback test | |
| `tests/skills_native_install.rs` | native-install integration test | stays if the CLI keeps `tina4 skills`; see below |
| `tests/skills_refresh_reports_why_it_failed.rs` | failure-reporting test | same |

Carry the `.gitattributes` rules for `install-skills.ps1` (the `binary` line and the
`*.sh text eol=lf` line) into `tina4-skills`. The Authenticode signature covers exact
bytes; a CRLF rewrite on checkout would void it.

### Stays in `tina4` (the CLI repo)

| File | Why it stays |
|------|--------------|
| `src/skills.rs` | the CLI still owns the `tina4 skills` / `tina4 ai` command; only its URL defaults change |
| `tests/skills_*.rs` | if the CLI keeps the command, its tests stay with it |
| `install.sh` / `install.ps1` | the CLI binary bootstrap installers - nothing to do with skills |
| `sign-mac.sh`, `scripts/sign-release.*` | sign the CLI binary and its bootstrap installers |
| `.github/workflows/ci.yml` frontmatter step | still validates the `tina4-cli` skill IF its source stays here (open question) |

### Code change in `src/skills.rs` (the only Rust change)

Two edits, both data-driven-preserving:

- `Config::manifest_urls()` CDN fallbacks change `tina4@<ref>` / `tina4/<ref>` to
  `tina4-skills@<ref>` / `tina4-skills/<ref>`.
- `repo_for_skill("tina4-cli")` changes its return from `"tina4"` to `"tina4-skills"`
  (only if the `tina4-cli` skill source moves - see open questions).

`skills.rs` stays data-driven, exactly as the frozen-138-bundle lesson requires: it still
reads the ref from the served installer (`parse_installer_ref`) and the file set from
`skills.sha256` (`fetch_manifest_entries`). No skill list is hardcoded in the client; the
only per-skill knowledge remains the `repo_for_skill` rule, and that rule only ever picks
a CDN-fallback host. Getting it wrong loses a fallback, never the primary tina4.com tier.
The three URL-shape unit tests (`src/skills.rs:635-665`) must be updated to the new host
and must stay byte-exact against `install-skills.sh` `skill_urls` and `install-skills.ps1`
`Get-Tina4SkillUrls`.

### Installer default-ref and fallback URLs

- The embedded `ref="${TINA4_SKILLS_REF:-3.13.156}"` default (`install-skills.sh:29`) and
  its PowerShell twin stay the same shape; only their value tracks the new tag line.
- The CDN roots `TINA4_SKILLS_JSDELIVR_ROOT` / `TINA4_SKILLS_RAW_ROOT` keep pointing at
  `tina4stack`, so only the per-file `repo` component (`tina4` -> `tina4-skills`) changes
  in `skill_urls`/`verify_checksums` for the manifest and the `tina4-cli` skill. The
  framework dev skills and the tina4-python shared skills keep their repos.
- `SKILLS_INSTALL_URL` (`tina4.com/install-skills.sh`) does not change. It is served by
  Jenkins from `tina4-documentation/docs/public/install-skills.sh`, repo-independent.

## Backward compatibility for already-installed clients

An installed client that has not updated still fetches from the old locations. The plan
keeps those alive rather than breaking them:

- **3.8.106 and newer**: unaffected by #141 at any time (they resolve latest from the
  `releases/latest` redirect, which only ever points at a CLI release). After the move,
  their primary tina4.com tier keeps working with no change, because tina4.com is
  repo-independent. Their CDN fallback for the manifest and `tina4-cli` would miss a brand
  new ref until they next update - but the primary tier serves it, so the install still
  completes. Acceptable, and it is only the fallback.
- **pre-3.8.106 (the shrinking 3.8.104 cohort)**: the move stops making their problem
  worse. Once no new bare `3.x` tag lands on the CLI repo, `releases.atom` there stops
  gaining skills entries, and each such client resolves to a real CLI release again and
  self-heals on its next `tina4 update`.
- **Old bare tags on the CLI repo stay put.** Do not delete `3.13.133 .. 3.13.156` from
  `tina4` (ISO control: never move or delete a tag). They keep serving the historical refs
  for any client pinned to them. The cutover only stops *adding* new ones.
- **The tina4.com bundle is the compatibility anchor.** Because the primary tier is
  tina4.com and it is repo-independent, no installed client has a hard dependency on which
  GitHub repo hosts the bundle. That is what makes the move safe.

## EV signing against the new repo

Signing is unchanged in mechanism, only in location:

- `install-skills.ps1` keeps its Code Infinity EV Authenticode signature, produced by
  SimplySign (cloud HSM, the private key never leaves the card) through jsign
  (`scripts/sign-skills-installer-mac.sh`) on macOS, or signtool
  (`scripts/sign-installers.ps1`) on Windows. The jsign path is the one in use here - not
  osslsigncode, whose libp11 engine path is missing on this setup.
- After the move, the signer runs in the `tina4-skills` checkout against that repo's
  `install-skills.ps1`. The signed bytes are then committed there and the bare skills tag
  is cut from `tina4-skills`.
- CI parity check moves too: the `windows-latest` job that does
  `Get-AuthenticodeSignature` on the checked-out and the raw-hosted `install-skills.ps1`
  runs in `tina4-skills` CI, fetching the raw URL from `tina4stack/tina4-skills/<sha>`
  instead of `tina4stack/tina4/<sha>`.
- `install.ps1` (the CLI binary bootstrap) keeps its own signature in the `tina4` repo,
  signed by the release pipeline. The two signing lines never merge.

## CI that moves with the bundle

The skills CI currently lives in `tina4/.github/workflows/ci.yml` (the `skills-installer`
matrix job and the strict-YAML frontmatter step). It moves to `tina4-skills` CI, and the
two open follow-ups fold in at the same time so they land once, in the right repo:

- **#112 strict-YAML frontmatter.** `gen-skills-bundle.sh` already runs
  `validate-skills-frontmatter.py` over every staged `SKILL.md`, and `ci.yml` runs it on
  this repo's `.claude/skills`. In `tina4-skills` CI, run it on the skills source on every
  PR that touches `.claude/skills/**` or the generators, with PyYAML as a CI-only tool.
- **#109 extend the regression guard to reference files.** `check-skills-regression.sh`
  already walks every published file under each skill, not just `SKILL.md`
  (`check-skills-regression.sh` `find "$dir" -type f`). Confirm that behaviour is the
  intended #109 scope and close #109 against it, or extend as #109 asks.
- **#110 run the regression guard automatically on skills PRs.** Add a CI job in
  `tina4-skills` that stages a throwaway bundle from the PR HEAD and runs
  `check-skills-regression.sh` against the currently published bundle
  (`tina4.com/skills/latest/` or the docs repo's `docs/public/skills/<latest>`). This is
  the guard #110 asks for, and `tina4-skills` is its natural home because the skills source
  now lives there.
- The `skills_installer_http.py` retry/fallback test moves with the installers.

## Tag and ref migration

- **New tags cut from `tina4-skills` only.** The skills tag line is bare `3.x` and keeps
  tracking the framework version for now (the pin is `3.13.x`, independent of the CLI's
  `3.8.x`), unless the maintainer decides to give it a fresh independent line (open
  question). Either way, the tag lands on `tina4-skills`, never on `tina4`.
- **Existing refs keep resolving.** For any ref already published (`3.13.133 .. 3.13.156`),
  the tina4.com bundle under `docs/public/skills/<ref>/` already serves it flat, so a
  pinned `TINA4_SKILLS_REF=3.13.151` install keeps working through the primary tier with no
  GitHub tag at all. The CDN fallbacks for those historical refs keep resolving against the
  old `tina4` bare tags, which stay in place.
- **The last CLI-repo bare tag (`3.13.156`).** Leave it. It is the current pin's tag and
  removing it would break the CDN fallback for every client still on `3.13.156` and violate
  the no-delete-a-tag ISO control. The first release after the cutover creates its bare tag
  on `tina4-skills` instead, and the pin bumps to that version.
- **`bump-skills-ref.sh`** updates its step list: the "create the bare `$version` tag in
  tina4 and all framework repos" instruction drops `tina4` and names `tina4-skills` as the
  one repo that carries the skills tag (framework repos still get their own release tags).

## Phased, reversible cutover

Each phase is independently shippable and has a rollback. No phase deletes anything from the
CLI repo until the new path is proven.

**Phase 0 - create and seed `tina4-skills` (additive, zero risk).**
- Create `tina4stack/tina4-skills` (maintainer decision: name, visibility).
- Copy in: the installers, the manifest, the `scripts/*skills*` set, the `tina4-cli` skill
  source, the installer tests, and the `.gitattributes` rules. Keep the originals in `tina4`.
- Cut a bare skills tag on `tina4-skills` matching the current pin (`3.13.156`), and confirm
  `cdn.jsdelivr.net/gh/tina4stack/tina4-skills@3.13.156/skills.sha256` resolves.
- Rollback: delete the unused new repo. Nothing in production points at it yet.

**Phase 1 - teach the generators and the bundle about the new source.**
- Point `scripts/skills-list.sh` / `gen-skills-*.sh` at the `tina4-skills` checkout for the
  manifest and the `tina4-cli` skill (framework and tina4-python skills unchanged).
- Re-stage the tina4.com bundle for a test ref and prove it round-trips byte-identically
  from the new source (`gen-skills-bundle.sh` already enforces
  `diff skills.sha256 bundle/skills.sha256`). This is issue #107's acceptance test.
- Rollback: revert the generator pointer; the committed manifest is unchanged.

**Phase 2 - point the clients' CDN fallbacks at `tina4-skills`.**
- Edit `src/skills.rs` (`manifest_urls`, and `repo_for_skill("tina4-cli")` if the skill
  moved), `install-skills.sh` (`verify_checksums`, and the `tina4-cli` entry's repo),
  `install-skills.ps1` (same), and the three URL-shape unit tests.
- Ship this in a CLI release (`v3.8.x`). The primary tina4.com tier already served the
  bundle, so this release only redirects the fallback. Old clients are unaffected.
- Rollback: the fallback URL change is one revert; the primary tier never depended on it.

**Phase 3 - cut the next skills release from `tina4-skills` only.**
- The next bundle (`3.13.157`) cuts its bare tag on `tina4-skills`, not `tina4`. Bump the
  pin with `bump-skills-ref.sh` after the tag is live.
- Verify `tina4/releases.atom` gains no new bare tag, and a fresh pre-3.8.106 client now
  resolves to the latest `v3.8.x` CLI release.
- Rollback: if a fallback regresses, re-point to the old `tina4` bare tag for that ref; the
  primary tier keeps serving regardless.

**Phase 4 - retire the skills files from the CLI repo (only after Phase 3 is proven).**
- Delete the moved files from `tina4` (installers, manifest, `scripts/*skills*`, the
  `tina4-cli` skill source if moved, installer tests). Remove the skills CI steps from
  `tina4/ci.yml`. Leave `src/skills.rs` (the command stays) and the historical bare tags.
- Rollback: `git revert` restores the files; the tags were never touched.

## Decisions that need the maintainer

1. **Repo name**: `tina4stack/tina4-skills` as written in #107, or another name?
2. **Visibility**: public (consistent with the rest of the stack, and the CDN fallbacks
   need public raw/jsDelivr access) or private with a mirror?
3. **Does the `tina4-cli` skill source move too?** It documents the CLI, so it has a natural
   home in `tina4`. But if it stays in `tina4`, its CDN fallback still needs a bare tag on
   the CLI repo, which re-creates the exact #141 defect. Recommendation: move it to
   `tina4-skills` so no bare tag ever lands on `tina4`; the skill content is still the CLI's
   to edit, through a PR to `tina4-skills`.
4. **Do design/seo/a11y (and js/maintainer/architect) ALSO leave tina4-python?** #107's
   original wording asks this. Moving them decouples them from the framework cadence and
   gives them one visible home. It is not required for the #141 fix (only the CLI-repo tag
   is), so it can be a later phase. Recommendation: do the #141-critical move first
   (manifest + tina4-cli), sequence the shared-skills move as Phase 5.
5. **Independent skills version line, or keep tracking `3.13.x`?** #107 suggests its own
   line. A fresh line (e.g. `skills-v1.x`) is cleaner but means teaching `bump-skills-ref.sh`
   and the pin parser a second shape. Keeping `3.13.x` is zero parser change. Recommendation:
   keep `3.13.x` through the cutover, revisit an independent line once the move is stable.
6. **Timing**: #107 says sequence this before the components work (tina4-js / developer
   skills / the tina4-design standard), since that touches multiple skills. Confirm the
   window.

## Open questions

- Does CI in `tina4-skills` have network access to tina4.com / the docs repo for the #110
  baseline? #110 already flags this as a requirement.
- Should the CLI keep a hard offline floor (`DEFAULT_REF`) pointing at a ref that is
  guaranteed to exist on `tina4-skills`? It currently reads `3.13.147`
  (`src/skills.rs:47`); confirm the floor ref is one that will exist in the new repo.

## References (ground truth read for this plan)

- tina4#141 close comment (the confirmed mechanism).
- tina4#107 body (the proposal and acceptance criteria).
- tina4#109, tina4#110, tina4#112 (the CI follow-ups folded in).
- `src/skills.rs:42-152` (`DEFAULT_REF`, `SKILLS_INSTALL_URL`, `repo_for_skill`,
  `Config::manifest_urls`, `skill_urls`, `resolve_ref`, `parse_installer_ref`).
- `install-skills.sh:29,153,182,237-249` (ref default, `skill_urls`, `verify_checksums`,
  the `tina4-cli` entry).
- `scripts/skills-list.sh`, `gen-skills-bundle.sh`, `gen-skills-sha256.sh`,
  `bump-skills-ref.sh`, `check-skills-regression.sh`, `validate-skills-frontmatter.py`,
  `sign-skills-installer-mac.sh`, `sign-installers-mac.sh`, `sign-installers.ps1`.
- `.gitattributes` (the `install-skills.ps1 binary` and `*.sh text eol=lf` rules).
- `.github/workflows/ci.yml` (the `skills-installer` job and the frontmatter step).
