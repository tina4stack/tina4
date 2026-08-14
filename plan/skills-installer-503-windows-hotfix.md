# Task: Skills installer 503 and Windows PowerShell hotfix

**Outcome:** `tina4 ai` and the documented installers survive transient GitHub failures on macOS,
Linux, and Windows PowerShell 5.1 without requiring a new Tina4 client release.

## Scope
- [x] Reproduce and identify the PowerShell 5.1 parameter failure
- [x] Add a PowerShell 5.1-compatible retry downloader
- [x] Add an independent CDN fallback for every skill file
- [x] Harden the `tina4.com` bootstrap launchers
- [x] Make the tina4-js skill trigger on `tina4js` and `Tina4 JS` spellings
- [x] Preserve staged, all-or-nothing publication
- [x] Sign the canonical PowerShell installer
- [ ] Push the canonical installer and documentation mirror

## Platforms
| Contract | macOS/Linux | Windows PowerShell 5.1 |
|---|---|---|
| Retry transient 503 | ✅ | ⚠️ Native CI pending |
| Fall back from GitHub Raw | ✅ | ⚠️ Native CI pending |
| Atomic installation | ✅ | ✅ |

The exact signed PowerShell installer passes retry and fallback contracts under
PowerShell 7 in the Linux lab container. Native Windows PowerShell 5.1 remains the
publication gate in GitHub Actions.

## Tests
- [x] Real local HTTP server returns 503, then succeeds
- [x] Real local HTTP server keeps the primary down and the fallback succeeds
- [x] Six skills, all references, and the version marker are installed
- [ ] Windows CI executes the real script using `powershell.exe`
- [ ] Live lab installation succeeds through `tina4.com`

## Bugs
- [x] SKILLS-PS51-UNSUPPORTED-RETRY-PARAMETERS
- [x] SKILLS-BOOTSTRAP-NO-RETRY
- [x] SKILLS-RAW-SINGLE-SOURCE

## Commits
- pending

## Status: In Progress
