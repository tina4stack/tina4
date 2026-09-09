# Task: Serve Tina4 skills from tina4.com (3-tier installer source)

## Why
GitHub `raw.githubusercontent.com` is the PRIMARY skills source and it 503s during
GitHub incidents. The installer already falls back to jsDelivr + verifies sha256, so
installs still complete, but the common path is noisy and slow while raw is degraded.
Owner chose: publish a versioned, sha256-verified skills bundle to tina4.com (own infra,
Jenkins-deployed from `docs/public`) and make the installer try **tina4.com -> jsDelivr
-> raw**. The common path then never touches GitHub; if tina4.com is ever down it still
falls back, and the sha256 manifest still gates every file.

## Design
- tina4.com bundle root: `https://tina4.com/skills/<ref>/`
  - Skill files served FLAT (stage-relative): `.../skills/<ref>/<skill>/SKILL.md`,
    `.../skills/<ref>/<skill>/references/<file>` -- identical layout to what the
    installer stages, so the bundle is a perfect round-trip and one shared file list
    drives both the manifest and the bundle (no third copy to drift).
  - Manifest: `.../skills/<ref>/skills.sha256` (byte-identical to committed tina4/skills.sha256)
  - Inner installers: `.../skills/<ref>/install-skills.sh` + `.ps1` (so the bootstrap
    wrapper can fetch the inner installer from tina4.com first too)
- Installer per-file URL, three tiers in order:
  1. tina4.com  `${tina4_root}/${ref}/${skill}/${rel}`                       (flat)
  2. jsDelivr   `${jsdelivr_root}/${repo}@${ref}/.claude/skills/${skill}/${rel}`
  3. raw        `${raw_root}/${repo}/${ref}/.claude/skills/${skill}/${rel}`
  ( `rel` = `SKILL.md` or `references/<file>` )
- Env overrides (per-tier, each owns its shape): `TINA4_SKILLS_TINA4_ROOT`,
  `TINA4_SKILLS_JSDELIVR_ROOT`, `TINA4_SKILLS_RAW_ROOT`.
- Checksum gate unchanged: refuse install on any mismatch/missing tool. tina4.com serving
  stale bytes -> checksum fails that file -> installer falls through to jsDelivr/raw. Self-correcting.

## Scope
- [x] Read installer (.sh/.ps1), bootstrap (.sh/.ps1), gen-skills-sha256.sh, bump-skills-ref.sh
- [x] `scripts/skills-list.sh` -- shared canonical (repo|skill|refs) list, sourced
- [x] Refactor `scripts/gen-skills-sha256.sh` to source the shared list
- [x] `scripts/gen-skills-bundle.sh` -- stage bundle into tina4-documentation/docs/public/skills/<ref>/
- [x] Refactor `tina4/install-skills.sh` -> 3-tier (tina4.com flat -> jsDelivr -> raw)
- [x] Refactor `tina4/install-skills.ps1` -> 3-tier  (RE-SIGN required after edit)
- [x] Refactor `docs/public/install-skills.sh` bootstrap -> 3-tier (tina4.com -> jsDelivr -> raw)
- [x] Refactor `docs/public/install-skills.ps1` bootstrap -> 3-tier
- [x] Generate the bundle locally; verify hashes == committed skills.sha256
- [x] End-to-end sandbox test: local HTTP server as tina4.com proves tier-1 hit,
      tier-2 fallback (tina4.com 404), tier-3 fallback, and checksum refusal
- [ ] Release cut (owner + SimplySign): bump to next ref, re-sign ps1, tag tina4 + 4
      framework repos, deploy bundle + bootstraps to tina4.com

## Tests (real, no mocks)
- [x] installer resolves tier-1 (tina4.com) when present -- served by a real local HTTP server
- [x] installer falls back to jsDelivr when tina4.com 404s (real 404 from the test server)
- [x] installer falls back to raw when tina4.com + jsDelivr both fail
- [x] checksum mismatch on a served file -> refuse install, nothing written
- [x] bundle skills.sha256 == committed tina4/skills.sha256 (byte-identical)

## Release strategy (decided)
Deliver the new installer + bundle via tina4.com at the EXISTING content ref
3.13.135 -- NO framework re-tags, NO package publishes. Rationale: each framework's
publish.yml triggers on any `[0-9]*.*.*` tag but GUARDS on tag == manifest version
== runtime __version__, so a bare 3.13.136 tag without a real code bump either
publishes 4 needless packages (version-string-only) or fails the guard (red CI).
The 47 skill files at tina4.com/skills/3.13.135 are checksum-identical to the
immutable 3.13.135 content, so content reproducibility holds; only the installer
SCRIPT improves (2-tier -> 3-tier), served fresh from tina4.com. Doctor stays
honest (bootstrap ref 3.13.135 == installed marker 3.13.135 -> Current). A future
real 3.13.136 framework release picks up the new installer automatically (it is on
tina4 main). Bonus fixed in-flight: doctor currency was silently broken (a refactor
removed the TINA4_SKILLS_REF marker from the bootstrap); restored + parser hardened.

## Bugs
- [x] doctor currency broken: bootstrap lost its `TINA4_SKILLS_REF:-` marker, so
      parse_ref_from_installer returned None (latest = benign-unknown). Fixed by
      giving the bootstrap a single `ref="${TINA4_SKILLS_REF:-<v>}"` pin; hardened
      the CLI parser to skip a marker mention with no version + 2 lock-in tests.

## Commits
- tina4 main `3ca8c23`  installers 3-tier + scripts + doctor fix (pushed)
- tina4-documentation main `2edd764`  bundle + bootstraps + .gitattributes + audit skip (pushed)

## Release steps (DONE)
1. [x] re-signed tina4/install-skills.ps1 (EV; SimplySign lapsed twice, re-auth then Succeeded;
       osslsigncode verify ok, digest match)
2. [x] committed + pushed tina4 main (no new bare tag)
3. [x] regenerated bundle into the real tree
4. [x] committed + pushed tina4-documentation main -> Jenkins deployed to tina4.com (~4.3 min)
5. [x] verified live:
       - tina4.com/skills/3.13.135/skills.sha256 == committed manifest (47 files)
       - bootstrap is the new 3-tier + ref pin
       - served install-skills.ps1 byte-identical to the signed source (Windows sig intact)
       - real `curl tina4.com/install-skills.sh | sh` -> 47 verified, 7 skills, marker 3.13.135
       - with BOTH GitHub tiers dead, tina4.com ALONE installs all 7 (self-sufficient)

## Traps caught (see [[reference_signed_served_bundle_traps]])
- git normalized CRLF->LF on the committed bundle -> would have broken the ps1 signature AND
  the skill checksums. Fixed: docs/.gitattributes `docs/public/skills/** binary`.
- `docs/public/`-only push STILL triggered the Jenkins deploy (~4.3 min) -- the old
  "public-only push doesn't deploy" was the VitePress/Apache era; tina4press+Jenkins deploys
  on any main push.

## Status: DONE + LIVE. tina4.com is the primary skills source; jsDelivr/raw are fallbacks.
