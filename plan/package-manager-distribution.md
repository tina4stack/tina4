# Task: Distribute the tina4 CLI through OS package managers

Make `tina4` installable through native package managers, not just the
`install.ps1` / `install.sh` one-liners and `cargo install`. Every channel feeds
from the **same GitHub Release artifacts** (the EV-signed `.exe`, the Linux/macOS
binaries, and `SHA256SUMS`) — one release pipeline, thin per-manager manifests
kept in sync by automation.

Ground truth for a manifest is three values, all on the published release:
**version** (the tag), **download URL** (`releases/download/v<ver>/<asset>`), and
**SHA256** (from `SHA256SUMS`). The release is published by the LOCAL sign step
(`scripts/sign-release.ps1` / `sign-mac.sh`), which regenerates `SHA256SUMS` over
the signed `.exe` — so downstream manifests must update on `release: published`,
never at build time (the build's draft `.exe` is unsigned and re-hashed later).

Decisions (2026-08-20, from the maintainer):
- **apt**: baseline signed `.deb` attached to each Release now (`cargo-deb`);
  `apt install ./tina4_x.y.z_amd64.deb`. Hosted `apt.tina4.com` repo is a
  documented follow-up, not this task.
- **Windows**: all three — Scoop, Chocolatey, winget — built ready-to-wire, with
  a `release-published` workflow that auto-bumps + publishes each. Publish steps
  skip gracefully until the accounts/repos/secrets exist.

## Scope
- [x] `.deb` packaging: `[package.metadata.deb]` in Cargo.toml + `.deb` build folded into
      release.yml's single-writer `release-assets` job (amd64 + arm64, `cargo deb --no-build`,
      flows through SHA256SUMS + the sign step's re-hash). Depends pinned `libc6` (cross-arch safe)
- [x] Scoop: `packaging/scoop/tina4.json` manifest (autoupdate) + bucket-push automation
- [x] Chocolatey: `packaging/chocolatey/` (`tina4.nuspec` + `tools/chocolateyinstall.ps1`
      + `chocolateyuninstall.ps1` + VERIFICATION.txt + LICENSE.txt) + `choco pack`/`push` automation
- [x] winget: `packaging/winget/` (version + installer + defaultLocale YAML) + official `wingetcreate` PR automation
- [x] `release-published.yml` workflow: render job (one source of truth) commits manifests
      back to main + secret-gated publish-{scoop,homebrew,choco,winget} jobs
- [x] Fix the STALE Homebrew formula (was pinned 3.0.0) — render de-stales it + tap-push job
- [x] Docs: `packaging/README.md` (authoritative channel guide), README install section,
      `scripts/RELEASING.md` downstream section, CLAUDE.md pointer, LICENSE (was missing)

## Parity (channel coverage)
| Channel | Platform | Manifest built | Auto-publish wired | Needs external one-time |
|---------|----------|----------------|--------------------|-------------------------|
| cargo   | all      | n/a (exists)   | ✅ in release.yml   | — |
| Homebrew| mac/linux| ✅ de-staled → 3.8.77 | ✅ tap-push (gated) | `tina4stack/homebrew-tap` + `HOMEBREW_TAP_TOKEN` |
| Scoop   | Windows  | ✅             | ✅ bucket-push (gated) | `tina4stack/scoop-bucket` + `SCOOP_BUCKET_TOKEN` |
| Choco   | Windows  | ✅             | ✅ pack/push (gated) | account (info@tina4.com) + `CHOCO_API_KEY` |
| winget  | Windows  | ✅             | ✅ wingetcreate (gated) | first `wingetcreate new` + `WINGET_TOKEN` |
| .deb    | Debian   | ✅             | ✅ in release.yml   | — (attached to release) |
| apt repo| Debian   | (follow-up)    | (follow-up)         | host + GPG key (documented) |

## Tests / verification (real, no mocks)
- [x] `cargo deb` builds a real installable `.deb` on the lab (Ubuntu, nvidia-rtx4500):
      `tina4_3.8.77-1_amd64.deb`, `Depends: libc6`; `dpkg -i` installs `/usr/bin/tina4`,
      `/usr/bin/tina4 --version` → `tina4 3.8.77`; `dpkg -r` removes it cleanly (DEB_BINARY_REMOVED_OK).
      (A pre-existing /usr/local/bin/tina4 from install.sh shadows PATH — unrelated to the .deb.)
- [x] Scoop manifest is valid JSON (`json.load` ok); autoupdate wired to SHA256SUMS regex
- [x] `choco pack` produces `tina4.3.8.77.nupkg` from the nuspec + tools
- [x] `winget validate packaging/winget` succeeds (only a benign PortableCommandAlias
      warning from the local client's older bundled schema; it is the correct field)
- [x] render reproduces exactly: idempotent at 3.8.77 (zero packaging diff vs the real
      published SHA256SUMS), and a 9.9.9 render propagates version+hash to all 8 manifests
      with no stale versions left, homebrew per-arch hash order correct
- [x] both workflows parse as valid YAML

## Bugs
- [x] Homebrew formula was STALE at 3.0.0 (never auto-bumped) — render now de-stales it, tap-push wired
- [x] Repo had NO LICENSE file despite `license = "MIT"` in Cargo.toml — added (cargo-deb/choco/crates want it)
- [x] render script initial BRE-vs-ERE bug (`\+`/`\{64\}` under `sed -E`) — caught by the 9.9.9 propagation test, fixed to ERE

## Commits
- 74ee343  feat(dist): package-manager distribution (Scoop, Choco, winget, .deb) + auto-publish — merged to main

## Status: DONE (merged to main, 74ee343). All manifests + automation built and locally verified. Remaining is the maintainer's one-time external setup — create the scoop-bucket + homebrew-tap repos, the chocolatey.org account, the first `wingetcreate new`, and add the four repo secrets (see packaging/README.md). Each publish job skips until its secret exists, so releases are safe now. Follow-up: hosted apt.tina4.com repo for `apt-get install tina4`.
