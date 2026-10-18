# Changelog

## 3.8.100 - 2026-10-18

- feat(install): Termux (Android) and iSH (iOS) are first-class. On Android the installer fetches a native, position-independent `tina4-android-arm64` build (built via the NDK, API 24) into `$PREFIX/bin` - the static musl binary that Android's loader rejects with `unexpected e_type: 2` is never used; on iSH it installs the static i686 musl build. Both self-update in place (#91).
- fix(env): a UTF-8 byte order mark at the start of a `.env` file no longer hides the first variable (#89).
- fix(install): the PHP install hint points at `tina4stack/tina4php`, the published Composer package (#87).
- serve: forwards `TINA4_PORT` and `TINA4_HOST` to the framework.

## 3.8.99 - 2026-10-15

- feat(cli): `tina4 uninstall` removes everything the client put on the machine - the config and credentials, the installed Tina4 AI skills across Claude, Codex and Cursor, and the binary itself. It lists every target first and asks before it removes anything; `--dry-run` only reports and `--yes` skips the prompts. The current project's own `.tina4/`, `.tina4-metrics.json` and `.tina4-docs` are a separate, explicit opt-in, so an uninstall never searches the rest of the filesystem (#84).
- refactor: a code-quality batch across the metrics engine, doctor, watcher and serve supervision - import resolution, Halstead scoring and filesystem-event handling split into focused helpers, and the dead respawn machinery removed - with no behaviour change (#73-#83).

## 3.8.96 — 2026-10-04

- fix(skills): `tina4 skills` / `tina4 update` now install the CURRENT published skills. The native installer carried a hardcoded 8-skill list and a baked `3.13.138` ref, so it could neither advance to a newer ref nor install a newly added skill - it attempted an 8-skill install against the 9-skill `3.13.146` ref and failed checksum. It is now data-driven: the ref is resolved from the served `install-skills.sh` (so a skills release reaches `tina4 update` with no CLI release), and the exact file set is derived from the published `skills.sha256` manifest (any skills, any one-level-nested references). CDN-fallback repo routing is a rule, not a list, so a new skill needs no change here and a wrong guess only loses the fallback, never the primary tier. Guards added so the installer can never again drift from the manifest (#60).

## 3.8.95 — 2026-09-30

- feat(metrics): `tina4 metrics --fail-on-regression` — a ratchet CI gate that exits 1 when a scan is measurably worse than the committed `.tina4-metrics.json` baseline: a new offender file, more offenders on a file, a worse worst-case complexity on a file that still offends, or more duplicated lines. It reuses the same "Since last run" classification, so it never fails on movement the report already prints as clean or improved. It is read-only on the baseline — a check never advances the ratchet, so re-baselining stays a deliberate plain `tina4 metrics` run you then commit — warns and passes when there is no baseline, and cannot combine with `--no-history`. Preferred over `--fail-on error` for a cross-framework gate, whose absolute severity bar fires on inherent file-size maintainability and pre-existing sub-40 complexity (#55).

## 3.8.94 — 2026-09-29

- fix(deploy): the generated deploy images pin every base image by digest, run as a non-root user, and stop baking secrets into layers. A build no longer carries a `latest` tag it cannot pin down, and a secret handed in at build time won't survive into the shipped image (#42).
- fix(release): self-update now verifies each download against its checksum before it swaps the binary, and refuses a downgrade instead of walking backwards. When Windows signing runs it patches only the signed exe's checksum line and leaves the others alone. The publish jobs wait behind a protected `release` environment (ADR-0081) (#40).
- fix(installers,scaffold): installs fail closed — the installer checks the checksum first and only then promotes the download into place. Each scaffold now writes its own random `TINA4_SECRET` instead of a shared placeholder, so a fresh project can't ship the same guessable secret as every other one (F8) (#41).
- fix(lint): the async-route lint flags a synchronous database call inside an `async` route (ADR-0074), the thing that actually blocks the loop, instead of the old heuristic that just demanded every handler be async (#39).
- chore(packaging): sync the Scoop, Homebrew, Chocolatey, and winget manifests to the release (#51).

## 3.8.93 — 2026-09-28

- fix(skills): the AI-skills install/refresh now runs fully in-process (native Rust: fetch + `ring` SHA-256 verify + direct writes) instead of shelling out to PowerShell to `iex` a downloaded script. On Windows that PowerShell-download-execute shape from a freshly-updated binary was blocked by Defender/ASR with `os error 225` ("the file contains a virus or potentially unwanted software"), which broke `tina4 update`'s skills refresh; the tina4 binary itself was always correctly EV-signed. No new crates.
- fix(mcp): the generated `.mcp.json` now uses the Streamable HTTP transport (`"type":"http"`, `/__dev/mcp`) instead of the dropped legacy `sse` transport, so modern Claude Code attaches the project's live `/__dev/mcp` tools; a stale Tina4-written `sse` config is upgraded in place, a user's own config is left untouched.
- test(release): cover the SPDX SBOM generator with a real-run test (`tests/test_release_inventory.py`). It builds the inventory from the actual lockfile and asserts the document names the `tina4` package and its dependencies, and it proves the generator refuses an unsound graph. `scripts/release-inventory.py` gains a pure `build_document` seam so the trust boundary is testable without mocks; the emitted release bytes are unchanged.

## 3.8.92 — 2026-09-26

- fix(cli): `tina4 routes` now loads route files in a fresh `tina4 init` Python project — the project root is placed on PYTHONPATH for python delegations, so route modules resolve instead of failing with "No module named 'src'".
- fix(cli): `tina4 generate page|component` in a `tina4 init js` project now delegates to `npx tina4js` instead of `vite`, so scaffolding works instead of failing with "Failed to run vite generate".
- ci: add a 5-language generator-parity gate (matrix over python/php/ruby/nodejs) that runs init → generate model/crud/page/component → routes and asserts produced names/paths/routes/migrations against a committed contract fixture, with cross-language parity. The framework crud route/template double-pluralisation is encoded and tracked (XFAIL) until the framework fix ships.

## 3.8.91 — 2026-09-24

- Verify self-update downloads against mandatory SHA256SUMS before replacement and refuse downgrades. Pass Windows hash paths as data rather than PowerShell code.
- Fail closed when installer integrity data is absent; Windows promotes a temporary download only after checksum and Code Infinity Authenticode verification.
- Preserve checksum lines for unchanged release assets when signing Windows binaries.
- Generate Node examples using tina4-nodejs and migrate retired scoped dependencies through parsed JSON without changing unrelated fields.
- Validate both canonical PowerShell installer signatures in Windows CI. Pin the Zig release build tool and limit the build job to read-only repository access.

## 3.8.90 — 2026-09-24

- Update locked quinn-proto to 0.11.15 and rand 0.8 to 0.8.6 for RUSTSEC-2026-0185 and RUSTSEC-2026-0097. Audit the complete lockfile and transitive unsoundness advisories in PR CI and before release builds.

- Open at most one browser tab, only in development. Recognize all eight ADR-0070 CI variables, including false-like `no` and `off` values.
- Declare SQLite in generated Ruby Gemfiles and add it during update/upgrade. Keep Puma opt-in through the Gemfile; default Ruby serving and deployment no longer install it.
- Make the Docker run hint publish the image’s actual exposed port.
- Remove the withdrawn `TINA4_MAIL_TLS_INSECURE` setting and document mail encryption according to ADR-0071.
- Pin the AI skills installers to framework release 3.13.138, including measured agent-time estimates. Refresh all 48 file checksums and re-sign the PowerShell installer.
- Add contribution/security policies and DCO/CLA checks. Publish package-manager manifest updates through pull requests instead of direct pushes to main.
- Ship a locked SPDX 2.3 dependency inventory and third-party notices, including Debian packages and the CLI container. Require exact CI checksums and tag-bound provenance before signing any release; attest every draft asset including the unsigned Windows input.
- License first-party source under MPL-2.0 with Code Infinity’s separate commercial option. Preserve third-party licences and notices; check dependency licence choices against the reviewed repository policy in PR CI and release audit.
