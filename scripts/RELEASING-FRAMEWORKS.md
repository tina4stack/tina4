# Releasing Tina4 (frameworks + installer + CLI)

This is the runbook for a full Tina4 release. `scripts/RELEASING.md` (the
sibling document) covers ONLY the CLI signing step. This document covers
everything else and calls that document at the right moment.

Framework releases move seven repositories in lockstep. Miss one file and
the release either publishes a broken package to a public registry, or
looks green but hands developers stale skills. This runbook has caught the
release wrong four times in a row; every step below exists because a real
release broke on it.

## Order matters

Do the steps in this exact order. Each one produces a state the next step
reads. Reorder and you paint yourself into a corner where the fix means
retagging, which invalidates trust anchors and looks careless.

```
0. Preflight         verify every file lines up before touching a tag
1. Framework code    merge feature/release<ver> to v3 in the four repos
2. Framework tags    tag 3.13.X (bare) in each repo, push, watch publish
3. Installer         bump ref + assertion + bootstrap
4. Docs              landing page, release notes, book chapters
5. CLI (optional)    only when the Rust CLI needs new bytes
6. Verify            confirm every registry actually got the release
```

Only step 5 needs the SimplySign session. Steps 0 to 4 and 6 are just git
+ shell, so they can run in one sweep. Sign only when the CLI moves.

## 0. Preflight

Run this before ANY tag push. Every item is a lesson from a broken release.

**Framework repos** (Python, PHP, Ruby, Node.js):

- [ ] Version bumped in the manifest: `pyproject.toml` / `composer.json` /
      `<gem>.gemspec` / `package.json`
- [ ] `CHANGELOG.md` has an entry for the new version at the top, listing
      EVERY change that landed (audit-bug fixes, test-harness fixes, skill
      updates, docstring parity). The initial "release preparation" commit
      usually captures only the planned scope; the batch that lands in the
      week after usually adds four more things nobody documented.
- [ ] Node only: `package-lock.json` regenerated with every platform binary
      variant (`rm -rf node_modules package-lock.json && npm install
      --package-lock-only --ignore-scripts`). vitest 4.x pulls vite 7 pulls
      esbuild 0.28+ transitively, and if the lock does not have the platform
      entries CI fails `npm ci` with `Missing: @esbuild/freebsd-arm64@0.28.2
      from lock file` and half a dozen sibling errors.
- [ ] Lab full-suite green across all four frameworks under `sudo
      HOME=/home/andre` with `TINA4_REQUIRE_SERVICES=1` and the full env
      sourced from `/root/tina4-lab/lab-env.sh` and `lab-env-for.sh <fw>`.
      Zero fails, zero skips. Read the summary line, do not trust an exit
      code.

**Installer repo** (`tina4`):

- [ ] `install-skills.sh` -> `ref="${TINA4_SKILLS_REF:-<new-ver>}"`
- [ ] `install-skills.ps1` -> same ref bump
- [ ] `tests/skills_installer_http.py` -> the pinned assertion moves to the
      new ref (grep the file for the current version). This test exists to
      catch exactly this drift; it will fail CI loudly if you skip it, but
      only after you have already tagged.

**Documentation repo** (`tina4-documentation`):

- [ ] `docs/index.md` "Current framework release: <new-ver>" plus the "What's
      new" entry
- [ ] `docs/<lang>/36-releases.md` for python, php, ruby, nodejs. Same
      content (Tina4 is uniform) with any framework-specific caveat.
- [ ] `docs/public/install-skills.sh` bootstrap points at
      `tina4/<new-ver>/install-skills.sh` for BOTH `primary_url` and
      `mirror_url`. This is the file tina4.com serves; if the bootstrap
      still fetches the old tag, `curl -fsSL https://tina4.com/install-skills.sh
      | sh` installs the old skills no matter what else you fix.
- [ ] `pnpm docs:build` green
- [ ] `python3 scripts/audit-truth.py --strict` shows the landing check ✓
      for the new version

**Book repo** (`tina4-book`):

- [ ] `book-<n>-<lang>/chapters/36-releases.md` for all four books, same
      release note at the top. Rebase against origin/main before committing
      so PDF-regeneration commits from the mail bot do not conflict.

**CLI repo** (`tina4`) if the CLI is also releasing:

- [ ] `Cargo.toml` version bumped
- [ ] `Cargo.lock` regenerated to match (`cargo build --release`). This IS
      tracked; missing this bump fails every build target in CI with `cannot
      update the lock file because --locked was passed to prevent this`.
- [ ] `cargo build --release --locked` passes locally, so we know CI's
      `--locked` build will succeed.
- [ ] `./target/release/tina4 --version` reports the new version. `cargo
      test --release` does NOT relink the bin; run `cargo build --release`
      before trusting the number.

**Cross-cutting**:

- [ ] Every commit above lists co-authors: `Co-Authored-By: Claude Opus 4.7
      <noreply@anthropic.com>` and the relevant skill (`tina4-maintainer`
      for release plumbing, `tina4-developer-<lang>` for framework work).

If any preflight box is not ticked, STOP. The rest of the runbook depends
on all of them.

## 1. Framework code lands on v3

For each of the four framework repos:

```bash
cd .worktrees/release-3.13.<ver>/<repo>
git fetch origin v3
git checkout v3
git reset --hard origin/v3
git merge --no-ff feature/release3.13.<ver> \
  -m "Merge feature/release3.13.<ver> into v3"
git push origin v3
```

Reset to `origin/v3` before the merge is the load-bearing line. Your local
`v3` is almost certainly stale from prior sessions and merging into it
produces a merge commit with the wrong base.

## 2. Tag each framework, wait for the publish workflows

Frameworks use BARE version tags (`3.13.105`, not `v3.13.105`). The `v`
prefix is reserved for the CLI in the `tina4` repo.

```bash
for repo in tina4-python tina4-php tina4-ruby tina4-nodejs; do
  cd .worktrees/release-3.13.<ver>/$repo
  git tag -a 3.13.<ver> -m "Release 3.13.<ver>"
  git push origin 3.13.<ver>
done
```

Each tag push triggers the repo's `publish.yml` workflow. Watch them:

```bash
for r in tina4-python tina4-php tina4-ruby tina4-nodejs; do
  gh run list --repo tina4stack/$r --workflow publish.yml --limit 1
done
```

All four should be `completed success`. Common failures:

- **Node**: `npm ci` refuses because `package-lock.json` is missing
  platform binaries. Fix on `v3` (regenerate the lock), commit, delete the
  tag both locally and on origin (`git push origin :refs/tags/3.13.<ver>`),
  re-tag, re-push. npm has NOT published yet, so retag is safe. Do not use
  `--force`.
- **PHP tag verification**: `SOURCE_VERSION` in `Tina4/App.php::VERSION`
  must match the tag exactly. The publish workflow refuses to publish on
  mismatch.

Verify against the registries:

```bash
curl -sf https://pypi.org/pypi/tina4-python/3.13.<ver>/json | head -c 100
curl -sf https://rubygems.org/api/v1/versions/tina4.json | grep 3.13.<ver>
curl -sf https://registry.npmjs.org/tina4-nodejs | grep 3.13.<ver>
curl -sf https://packagist.org/packages/tina4stack/tina4-php.json | grep 3.13.<ver>
```

Packagist mirrors GitHub via webhook; give it a minute.

## 3. Installer

The two `install-skills.sh` files must move together. Skip either and
`curl -fsSL https://tina4.com/install-skills.sh | sh` still installs the
old skills.

```bash
# 3a. Bootstrap on tina4.com
cd tina4-documentation
sed -i.bak 's|tina4/3\.13\.<old>|tina4/3.13.<new>|g; \
            s|tina4@3\.13\.<old>|tina4@3.13.<new>|g' \
  docs/public/install-skills.sh
rm docs/public/install-skills.sh.bak
git add docs/public/install-skills.sh
git commit -m "install-skills: bootstrap fetches 3.13.<new> (was 3.13.<old>)"
git push origin main

# 3b. Real installer on tina4
cd ../tina4
sed -i.bak 's/ref="${TINA4_SKILLS_REF:-3\.13\.<old>}"/ref="${TINA4_SKILLS_REF:-3.13.<new>}"/' \
  install-skills.sh
sed -i.bak 's/"3\.13\.<old>"/"3.13.<new>"/g' tests/skills_installer_http.py
rm install-skills.sh.bak tests/skills_installer_http.py.bak
git add install-skills.sh tests/skills_installer_http.py
git commit -m "install-skills: bump ref 3.13.<old> -> 3.13.<new>"
git push origin main

# 3c. Tag the tina4 repo at the framework version (no v prefix here —
# the shell script installer track uses bare version tags to match what
# the bootstrap fetches; the CLI track uses vX.Y.Z tags separately).
git tag -a 3.13.<new> -m "Release 3.13.<new>"
git push origin 3.13.<new>
```

Jenkins deploys tina4-documentation on push to main. Verify the bootstrap
is live once the deploy lands:

```bash
curl -sf https://tina4.com/install-skills.sh | grep primary_url
```

## 4. Documentation

The docs and book chapters run behind the framework by design (they land
after the release engineer proves the release works), but must not lag
more than one version.

```bash
# Prepend the new release note to each book chapter + docs page.
# Same content in all 4 languages; Tina4 is uniform.

# Book (main branch)
cd tina4-book
# For each of book-{1-python,2-php,3-ruby,4-nodejs}/chapters/36-releases.md,
# awk-insert the new entry before the current first "## v3.13" heading.
git add book-*/chapters/36-releases.md
git commit -m "docs(book): add 3.13.<ver> release notes across all 4 books"
git pull --rebase origin main   # PDF-regen bot commits appear regularly
git push origin main

# Documentation site (main branch)
cd ../tina4-documentation
# Insert into docs/{python,php,ruby,nodejs}/36-releases.md AND
# bump docs/index.md "Current framework release" and "What's new" entry.
pnpm docs:build     # must be green
python3 scripts/audit-truth.py --strict   # landing check ✓
git add docs/*/36-releases.md docs/index.md
git commit -m "docs: 3.13.<ver> release notes; landing bumped"
git push origin main
```

Framework `CHANGELOG.md` files land on `v3` at any point (before or after
tag), since they are documentation, not code. If you add entries after
tagging (which is normal because the release-preparation commit rarely
captures every fix), commit them straight to `v3` without retagging.

## 5. CLI (only when Rust bytes change)

The CLI is on its own version track (`3.8.x` as of writing). A framework
release does not automatically move it. Do this section only when the CLI
itself needs new bytes.

```bash
cd tina4
# Bump the workspace version
sed -i.bak 's/^version = "3\.8\.<old>"/version = "3.8.<new>"/' Cargo.toml
rm Cargo.toml.bak

# Regenerate the lock and prove it satisfies --locked
cargo build --release
./target/release/tina4 --version   # must print the new version
cargo build --release --locked     # must succeed offline

git add Cargo.toml Cargo.lock
git commit -m "Release 3.8.<new>"
git push origin main

# Tag with the v-prefix (this is what release.yml triggers on)
git tag -a v3.8.<new> -m "Release 3.8.<new>"
git push origin v3.8.<new>
```

CI now builds 5 targets, publishes the crate to crates.io, and creates a
DRAFT release with the binaries + `SHA256SUMS`. `publish-crate` runs in
parallel with the binary builds, so a build failure does NOT stop the
crate publish. That means if you have to fix and retag, the second run's
`publish-crate` will fail with "already published" - this is expected and
does not block the retag from producing the artifacts.

Sign the draft. This step requires the SimplySign 2FA session:

```bash
# Open SimplySign Desktop and log in (your 2FA)
cd tina4
./scripts/sign-release.sh v3.8.<new>
```

The script downloads the draft assets, signs the Windows exe against the
Certum EV cert on the cloud HSM, repacks, regenerates `SHA256SUMS` over
the signed bytes, and publishes the release. See `scripts/RELEASING.md`
for the full signing story and gotchas.

## 6. Verify

Run every check. Do not trust a step until you have seen the artifact on
the public URL.

```bash
# Framework packages
curl -sfI https://pypi.org/pypi/tina4-python/3.13.<ver>/
curl -sf https://rubygems.org/api/v1/versions/tina4.json | grep 3.13.<ver>
curl -sf https://registry.npmjs.org/tina4-nodejs | grep 3.13.<ver>
curl -sf https://packagist.org/packages/tina4stack/tina4-php.json | grep 3.13.<ver>

# Installer bootstrap (Jenkins may take a minute to redeploy)
curl -sf https://tina4.com/install-skills.sh | grep primary_url
# Expect: tina4/3.13.<ver>/install-skills.sh

# CLI (if released)
curl -sf https://crates.io/api/v1/crates/tina4 | grep -m1 '"newest_version"'
# osslsigncode verify on the published exe (do not trust the local dist/)
gh release download v3.8.<new> --repo tina4stack/tina4 \
  --pattern tina4-windows-amd64.exe --dir /tmp/verify
osslsigncode verify /tmp/verify/tina4-windows-amd64.exe

# Docs site (Jenkins refreshes on tina4-documentation main push)
curl -sf https://tina4.com/ | grep -oE "Current framework release: 3\.13\.[0-9]+"
```

Every check must pass. If ANY check fails, the release is not done.

## Failure modes and what they teach

Each of these has bitten a release. The runbook exists so they only bite
once.

- **Node lockfile missing platform binaries**: vitest 4.1.9 -> vite 7.3.6 ->
  esbuild 0.28+. A lockfile generated on Apple Silicon before the vitest
  upgrade did not carry the `@esbuild/freebsd-*` and `@esbuild/linux-*`
  optional deps. CI's `npm ci` fails with `Missing: @esbuild/freebsd-arm64
  @0.28.2 from lock file`. Fix: regenerate the whole lock (`rm
  package-lock.json && npm install --package-lock-only`), commit, retag.

- **`Cargo.lock` not committed with the manifest bump**: 2026-08-19 memory
  said Cargo.lock was gitignored - it was not, but the note had rotted.
  Bumping `Cargo.toml` without `Cargo.lock` fails every build target with
  `cannot update the lock file because --locked was passed`. Fix: commit
  the lock, delete + recreate the tag.

- **Installer pin still at the previous version**: the every-release rule
  has been in `project_tina4_skills_drift` since 2026-07-03. It gets
  skipped. Symptom: `curl tina4.com/install-skills.sh | sh` still fetches
  the old skills after the framework release ships. Fix: bump both
  `install-skills.sh` and the bootstrap in `docs/public/install-skills.sh`,
  tag the tina4 repo at the framework version.

- **`tests/skills_installer_http.py` pin assertion stale**: this test pins
  the version the installer writes to `.tina4-skills-ref`. When the pin
  moves, the assertion must move with it. Missing this fails tina4 CI on
  the same commit that bumps `install-skills.sh`. Fix: bump the assertion
  in the same commit as the ref.

- **Book main branch moved forward while you were editing**: PDF-regen bot
  commits land on `tina4-book` main every few hours. `git pull --rebase`
  before pushing.

- **Landing page still says the old version**: `audit-truth.py --strict`
  has a landing check that fails when `docs/index.md` "Current framework
  release" lags behind the newest 36-releases.md entry. Fix in the same
  commit as the release notes.

- **PHP source version mismatch on publish**: `Tina4/App.php::VERSION` must
  equal the tag. The publish workflow refuses to publish on mismatch. Fix:
  bump the constant in `feature/release3.13.<ver>` before merging to v3.

## The runbook you want next: `scripts/framework-release.sh`

Every step above is machine-checkable. A companion script would:

1. Read the target version from the first argument
2. Verify all four framework repos have the version in their manifest
3. Verify `install-skills.sh` and its test assertion carry the target
   version
4. Verify `docs/public/install-skills.sh` bootstrap points at the target
   tag
5. Verify `docs/index.md` "Current framework release" and every book
   chapter's newest entry match
6. Verify `Cargo.lock` matches `Cargo.toml` when the CLI is releasing
7. If any check fails, print the exact fix line and exit 1
8. If all green, do the tag dance in order

That script does not exist yet. Write it after the next release.
