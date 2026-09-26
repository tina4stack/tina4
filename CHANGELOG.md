# Changelog

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
