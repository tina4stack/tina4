# ADR-0102: the skills bundle lives in its own repo so no bare tag lands on the CLI repo

- Status: proposed (STUB - to be filed in
  `tina4-documentation/plan/v3/decisions/ADR-0102.md` and indexed in `DECISIONS.md`
  once the maintainer accepts the #107 plan)
- Date: 2026-11-02
- Context: the AI-skills install path (`tina4/src/skills.rs`, `tina4/install-skills.sh`,
  `tina4/install-skills.ps1`, the `tina4/scripts/*skills*` generators), the release
  methodology (feature-branch-per-release, the tag publishes), tina4#107, tina4#141,
  tina4#106/#109/#110/#112.
- Supersedes: none. Hardens the skills-release process that #106 (the regression guard)
  and v3.8.106 (the `releases/latest` resolver) already patched around.

## Context

The skills bundle is served from three tiers: tina4.com first (flat, repo-independent),
then jsDelivr and raw.githubusercontent as CDN fallbacks. The fallbacks fetch the
`skills.sha256` manifest and the `tina4-cli` skill from a tag on the `tina4` CLI repo, so
the release process cuts a **bare `3.13.x` git tag on the CLI repo** every time the skills
change (`scripts/bump-skills-ref.sh`: "create the bare `$version` tag in tina4 and all
framework repos").

That bare tag has nothing to do with the CLI binary, but it lands on the CLI repo's tag
line next to the real `v3.8.x` releases. The pre-3.8.106 `tina4 update` resolver reads the
first `/releases/tag/` link in `tina4/releases.atom` as "latest". The skills tag
`3.13.156` tops that feed, so the client asks for `v3.13.156` binary assets, which do not
exist, and every asset 404s (tina4#141).

v3.8.106 changed the resolver to the `releases/latest` redirect, which excludes drafts and
only ever points at a real CLI release, so 3.8.106+ clients are immune. v3.8.113 restored
the tag ordering so the shrinking 3.8.104 cohort resolves correctly today. Neither removes
the cause: the next skills tag re-tops the feed and re-breaks that cohort. The cause is
that a non-CLI artifact is tagged on the CLI repo.

## Decision

The language-agnostic skills bundle gets its own repo, `tina4stack/tina4-skills`, and the
skills release tag is cut there, never on the `tina4` CLI repo.

- The `skills.sha256` manifest, both installers (`install-skills.sh`,
  `install-skills.ps1`), the generator and guard scripts, and the `tina4-cli` skill source
  move to `tina4-skills`.
- `src/skills.rs` changes its CDN-fallback hosts for the manifest and the `tina4-cli` skill
  from `tina4` to `tina4-skills`. It stays data-driven: the ref still comes from the served
  installer and the file set still comes from `skills.sha256`; no skill list is hardcoded.
- The primary tina4.com tier does not change, because it is flat and repo-independent, so
  every installed client keeps working through it with no dependency on which GitHub repo
  hosts the bundle. That is what makes the move backward-compatible.
- The CLI repo keeps only `v`-prefixed tags. Its `releases.atom` can never again name a
  skills version, which closes tina4#141 at the root for every client, old and new.
- The CI guards fold in at the new home: strict-YAML frontmatter (#112), the
  reference-file-aware regression guard (#109), and the automatic PR guard (#110) run in
  `tina4-skills`.

Existing bare tags on the CLI repo (`3.13.133 .. 3.13.156`) stay in place (the no-delete-a-
tag ISO control), so clients pinned to them keep resolving through the old fallbacks and
through tina4.com.

## Consequences

- **No breaking change for installed clients.** The primary tina4.com tier is unchanged, so
  every client installs and verifies exactly as before. Only the CDN fallback host for the
  manifest and the `tina4-cli` skill changes, and the fallback is only reached when
  tina4.com is unreachable.
- **tina4#141 is fixed at the root.** With no new bare `3.x` tag on the CLI repo, the
  ordering accident cannot recur, so the ordering holding-action (v3.8.113) and the
  resolver patch (v3.8.106) stop carrying the risk alone.
- **The skills bundle gets an independent cadence.** A framework release can no longer
  silently drop or shrink a language-agnostic skill, because the skills no longer ride a
  framework repo's release (tina4#107's acceptance criterion), on top of the regression
  guard that already backstops it.
- **EV signing is unchanged in mechanism.** `install-skills.ps1` keeps its Code Infinity EV
  Authenticode signature via SimplySign + jsign, produced and verified in the `tina4-skills`
  checkout, with the same `.gitattributes binary` rule protecting its signed bytes.
- **One more repo to maintain.** The skills source, its CI, and its release tag now live
  apart from both the CLI and the framework repos. The plan sequences the `tina4-cli` skill
  and, later, the shared skills (design/seo/a11y/js/maintainer/architect) into it so there
  is one visible home for language-agnostic skills.

## Open decisions deferred to the maintainer

Repo name and visibility; whether the `tina4-cli` skill source moves (recommended: yes, or
the CLI repo still needs a bare tag); whether the shared skills also leave `tina4-python`
(recommended: a later phase); and whether the skills version line stays `3.13.x` or becomes
an independent line (recommended: keep `3.13.x` through the cutover). See
`tina4/plan/107-skills-repo-split.md` for the full list and the phased cutover.
