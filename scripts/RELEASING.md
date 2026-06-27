# Releasing the tina4 CLI (signed)

The release is built and verified in CI, then **signed by a human at release
time**. The EV code-signing 2FA is entered by you each release; no signing
secret (OTP seed) is ever stored in CI, so nothing automated can produce a
Code Infinity signature.

## Trust chain

A signature proves origin, not goodness, so CI makes the artifact provably good
before any signature goes on:

1. **audit** - `cargo-deny` (advisories, bans, sources) gates the build.
2. **build** - `cargo build --locked` on a pinned Rust toolchain, all Actions
   pinned to commit SHAs, with a pre-sign smoke test.
3. **provenance** - SLSA build-provenance attestation for the Linux/macOS
   binaries (the Windows .exe is re-signed locally, so its trust anchor is the
   EV Authenticode signature instead).
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

**macOS / Linux (fallback):** see the header of `scripts/sign-release.sh` for
the `osslsigncode` + SimplySign PKCS#11 prerequisites, then:
```
sh scripts/sign-release.sh v3.8.53
```

Either script: downloads the draft's assets, signs the `.exe`, verifies it,
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
