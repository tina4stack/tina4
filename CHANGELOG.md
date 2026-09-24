# Changelog

## 3.8.90 — 2026-09-24

- Open at most one browser tab, only in development. Recognize all eight ADR-0070 CI variables, including false-like `no` and `off` values.
- Declare SQLite in generated Ruby Gemfiles and add it during update/upgrade. Keep Puma opt-in through the Gemfile; default Ruby serving and deployment no longer install it.
- Make the Docker run hint publish the image’s actual exposed port.
- Remove the withdrawn `TINA4_MAIL_TLS_INSECURE` setting and document mail encryption according to ADR-0071.
- Pin the AI skills installers to framework release 3.13.138, including measured agent-time estimates. Refresh all 48 file checksums and re-sign the PowerShell installer.
- Add contribution/security policies and DCO/CLA checks. Publish package-manager manifest updates through pull requests instead of direct pushes to main.
- Ship a locked SPDX 2.3 dependency inventory and third-party notices, including Debian packages and the CLI container. Require exact CI checksums and tag-bound provenance before signing any release; attest every draft asset including the unsigned Windows input.
- License first-party source under MPL-2.0 with Code Infinity’s separate commercial option. Preserve third-party licences and notices; check dependency licence choices against the reviewed repository policy in PR CI and release audit.
