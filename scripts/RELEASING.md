# Releasing the tina4 CLI (signed)

The release is built and verified in CI, then **signed by a human at release
time**. The EV code-signing 2FA is entered by you each release; no signing
secret (OTP seed) is ever stored in CI, so nothing automated can produce a
Code Infinity signature.

## Trust chain

A signature proves origin, not goodness, so CI makes the artifact provably good
before any signature goes on:

1. **audit** - `cargo-deny` (advisories, bans, sources, licences) gates the build.
2. **build** - `cargo build --locked` on a pinned Rust toolchain, all Actions
   pinned to commit SHAs, with a pre-sign smoke test.
3. **provenance** - SLSA attestations cover every draft asset, including the
   unsigned Windows input. Signing verifies the tag-bound attestations first;
   the final Windows bytes then carry the EV Authenticode signature.
4. **checksums** - `SHA256SUMS` over the artifacts.
5. CI publishes all of this as a **draft** release. The Windows .exe in the
   draft is unsigned until you finalize it.
6. **You sign** the Windows .exe locally (your 2FA), which regenerates
   `SHA256SUMS` over the signed bytes and publishes the release.

The installers (`install.sh`, `install.ps1`) verify the download against the
published `SHA256SUMS` before trusting it.

## Cutting a release

1. Bump the version in `Cargo.toml`, commit, and tag:
   ```
   git tag v3.8.53 && git push origin v3.8.53
   ```
   (Use a prerelease tag like `v3.8.53-rc.1` first to exercise the whole chain.)
2. Wait for the **Release Binaries** workflow to finish. It leaves a **draft**
   release with the five binaries, `SHA256SUMS`, and provenance attestations.
3. Sign and finalize (see below). This publishes the release.

## Signing (you enter the 2FA)

You need the cert's SHA1 **thumbprint** (read it in SimplySign: double-click the
cert, see Thumbprint). It is not a secret.

**Windows (primary):** open SimplySign Desktop and log in (your 2FA), then:
```
$env:CERT_THUMBPRINT = "<sha1-thumbprint>"
pwsh ./scripts/sign-release.ps1 -Tag v3.8.53
```

**macOS (proven): `sh sign-mac.sh <tag>` - use jsign, NOT osslsigncode.** v3.8.55
was signed on macOS this way. osslsigncode + the OpenSSL libp11 engine FAILS on
the SimplySign *cloud* module (the pkcs11 provider won't load; the legacy engine
path dies with "PKCS#11 module: Attribute type invalid" - the cloud HSM rejects
the RSA sign the engine issues). jsign talks PKCS#11 directly and signs cleanly.

```
# 1. Tools:
brew install jsign osslsigncode   # jsign signs; osslsigncode verifies

# 2. Open SimplySign Desktop and LOG IN (your 2FA; cloud card mounted).

# 3. Preview the resolved config WITHOUT signing (does not spend your session):
sh sign-mac.sh v3.8.56 --check
# 4. Sign + verify + checksum + publish. Everything is auto-discovered (module
#    symlink, cert alias from the token, chain from the token); override via
#    TINA4_PKCS11_MODULE / TINA4_SIGN_ALIAS / TINA4_TS_URL only if your install differs:
sh sign-mac.sh v3.8.56
```

`sign-mac.sh` resolves the SimplySign module symlink, writes a SunPKCS11 config,
and runs `jsign --storetype PKCS11 --storepass "" --alias <CKA_LABEL>
--tsmode AUTHENTICODE --tsaurl http://time.certum.pl/`. jsign reads the signing
certificate (and chain) from the token, so no cert PEM lives on disk. Read the
cert's CKA_LABEL (the `--alias`) with
`pkcs11-tool --module <module> --list-objects --type cert` (NO `--login` - the
cloud card has no PIN).

**Linux / physical-card fallback: `sh scripts/sign-release.sh <tag>`** (osslsigncode
+ libp11). This is the well-trodden path for a *physical* Certum card on Linux; it
does not work against the SimplySign *cloud* module (use `sign-mac.sh` there).

Either script downloads the draft's assets, signs the `.exe`, verifies it,
re-uploads it, regenerates `SHA256SUMS` over the signed bytes, and un-drafts the
release.

## Verifying a release (what to tell a security reviewer)

- **Windows:** the `.exe` carries an EV Authenticode signature from
  `Code Infinity (Pty)` - check Properties -> Digital Signatures, or
  `signtool verify /pa tina4-windows-amd64.exe`. EV gives immediate SmartScreen
  reputation.
- **Linux/macOS:** verify build provenance with
  `gh attestation verify <file> --repo tina4stack/tina4`.
- **All:** `sha256sum -c SHA256SUMS` (or `shasum -a 256 -c`) against the
  published checksums.

## Downstream package managers (automatic)

You do nothing extra. Two hooks fan the signed release out to every channel:

- **`.deb` (Debian/Ubuntu)** is built and attached during the release itself:
  the `release-assets` job packages the amd64 + arm64 `.deb` from the just-built
  glibc binaries (`cargo deb --no-build`) and includes them in `SHA256SUMS`. When
  you sign, the sums are regenerated over the `.debs` too.
- **Scoop, Homebrew, Chocolatey, winget** update when the release is *published*:
  `.github/workflows/release-published.yml` renders every manifest from the tag +
  `SHA256SUMS` (`scripts/render-manifests.sh`), commits the bumped manifests back
  to `main`, and pushes each channel. Each channel **skips** unless its secret is
  configured, so nothing breaks before a channel is live.

The full channel guide - install commands, per-channel one-time setup, and the
required secrets - is [`packaging/README.md`](../packaging/README.md). To preview
what a bump will publish: `bash scripts/render-manifests.sh <version> <SHA256SUMS>`
then `git diff -- packaging homebrew`.

## Release inventory and signing inputs (3.8.90 onward)

CI generates `tina4.spdx.json`, `LICENSE-INVENTORY.json` and
`THIRD-PARTY-NOTICES.txt` from the locked all-platform Cargo graph. This includes
build/development dependencies, so it is a conservative superset of each binary.
Cargo archive checksums identify the exact source crates. Missing/unknown licence
declarations or missing notice texts fail the inventory check on every PR. This
records upstream declarations and does not claim an individual legal opinion.
The separate `LICENSE-POLICY.md` allowlist is enforced by cargo-deny on every
pull request and release build.
For the crates whose archives omit licence files, reviewed texts from their exact
upstream source commits are stored under `scripts/third-party-licenses/`, with
source URLs and SHA-256 checksums.

The release ships these three assets next to the binaries, with notices and the
inventory also inside Debian packages and the CLI container. CI attests every draft asset and the
checksum manifest, including the unsigned Windows input. All three signing
scripts require Python 3 and call `verify-release-inputs.py` before signing:
checksum coverage must be exact, and every asset must have provenance bound to
this repository's release workflow and this exact release tag. No bypass flag
is provided. The final Windows binary uses its EV signature; final checksums
are regenerated after signing and preserve the SBOM/notices.

Local checks: `cargo test --locked`, `cargo clippy --locked -- -D warnings`,
`cargo build --release --locked`, `python3 scripts/release-inventory.py --output dist`,
and `python3 tests/release_integrity.py`.
