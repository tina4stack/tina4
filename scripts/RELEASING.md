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

**macOS (proven): `sh sign-mac.sh <tag>` - use jsign, NOT osslsigncode.** v3.8.55
was signed on macOS this way. osslsigncode + the OpenSSL libp11 engine FAILS on
the SimplySign *cloud* module (the pkcs11 provider won't load; the legacy engine
path dies with "PKCS#11 module: Attribute type invalid" - the cloud HSM rejects
the RSA sign the engine issues). jsign talks PKCS#11 directly and signs cleanly.

```
# 1. Tools:
brew install jsign osslsigncode   # jsign signs; osslsigncode verifies

# 2. Open SimplySign Desktop and LOG IN (your 2FA; cloud card mounted).

# 3. Sign + verify + checksum + publish (values are pre-filled; override via
#    TINA4_PKCS11_MODULE / TINA4_SIGN_ALIAS / TINA4_TS_URL if your install differs):
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
