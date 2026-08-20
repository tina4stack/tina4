# Distributing the tina4 CLI through package managers

One release, many channels. Every package manager below installs the **same
binaries** the project already builds and signs for each GitHub Release - no
channel has its own build. A release carries, per platform, three facts a
manifest needs: the **version** (the tag), the **download URL**
(`releases/download/v<version>/<asset>`), and the **SHA-256** (from the release's
`SHA256SUMS` asset). The automation renders every manifest from exactly those
facts, so the channels can never drift from each other or from the release.

If you only want the install commands, jump to [Installing](#installing-per-channel).
If you are wiring up a channel for the first time, read
[One-time setup](#one-time-setup-per-channel).

## How it fits into the release

The distribution work is split across the two release workflows and one local
step - it is part of the normal build, not a separate chore:

```
 tag v3.8.x pushed
        │
        ▼
 .github/workflows/release.yml   (CI, on tag)
   audit → build (7 targets) → release-assets
     • builds the amd64 + arm64 .deb from the just-built glibc binaries
       (cargo deb --no-build) and drops them in the draft
     • writes SHA256SUMS over every asset (binaries + .debs)
     • publishes a DRAFT release
        │
        ▼
 scripts/sign-release.ps1        (local, you enter the EV 2FA)
     • signs the Windows .exe, regenerates SHA256SUMS over the signed bytes
       (re-hashing the .debs too), and PUBLISHES the release
        │
        ▼
 .github/workflows/release-published.yml   (CI, on release: published)
   render → publish-{scoop,homebrew,choco,winget}
     • scripts/render-manifests.sh rewrites every manifest in packaging/ and
       homebrew/ from the published version + SHA256SUMS
     • commits the bumped manifests back to main
     • pushes each channel (each SKIPS unless its secret is set)
```

Why `release: published` and not the tag: the Windows `.exe` is EV-signed
**locally** at release time, which regenerates `SHA256SUMS` over the signed
bytes. Only after the sign step publishes the draft are the version, URL, and
hash final - so that is when the manifests are rendered.

## The render script - one source of truth

[`scripts/render-manifests.sh`](../scripts/render-manifests.sh) `<version>
<sha256sums-file>` rewrites every manifest to a release. It is pure text
substitution: no network, no build. CI calls it; you can too, to preview a bump:

```bash
# Preview what the next release's manifests would look like, against a real
# published SHA256SUMS:
curl -fsSL https://github.com/tina4stack/tina4/releases/download/v3.8.77/SHA256SUMS -o /tmp/S
bash scripts/render-manifests.sh 3.8.77 /tmp/S
git diff -- packaging homebrew      # review, then discard or commit
```

It updates: the Scoop manifest, the Chocolatey nuspec + install script +
VERIFICATION, all three winget manifests, and the Homebrew formula (version,
URLs, and every SHA-256, mapped per architecture).

## Channels

| Channel   | Platform        | Manifest in repo                          | Install command                                   | Auto-published by | One-time setup |
|-----------|-----------------|-------------------------------------------|---------------------------------------------------|-------------------|----------------|
| cargo     | any (from source)| `Cargo.toml`                             | `cargo install tina4`                             | `release.yml`     | crates.io token (done) |
| Homebrew  | macOS, Linux    | `homebrew/tina4.rb`                        | `brew install tina4stack/tap/tina4`               | `release-published` | tap repo + `HOMEBREW_TAP_TOKEN` |
| Scoop     | Windows         | `packaging/scoop/tina4.json`              | `scoop install tina4`                             | `release-published` | bucket repo + `SCOOP_BUCKET_TOKEN` |
| Chocolatey| Windows         | `packaging/chocolatey/`                    | `choco install tina4`                             | `release-published` | choco account + `CHOCO_API_KEY` |
| winget    | Windows         | `packaging/winget/`                        | `winget install Tina4Stack.Tina4`                 | `release-published` | first `wingetcreate new` + `WINGET_TOKEN` |
| .deb      | Debian/Ubuntu   | `Cargo.toml` `[package.metadata.deb]`      | `apt install ./tina4_<v>_amd64.deb`               | `release.yml`     | none |
| apt.tina4.com | Debian/Ubuntu | `packaging/apt/`                       | `apt-get install tina4`                           | `apt-publish.sh` (on server) | live — key on the tina4.com box |

`SHA256SUMS`, `cargo`, and `.deb` need nothing new. The rest each need a
one-time account/repo plus a repository secret; until that secret exists, the
corresponding publish job **skips cleanly** (the flag is `false`), so landing all
of this before the accounts are ready is safe.

## Installing (per channel)

```powershell
# Scoop (no admin needed)
scoop bucket add tina4 https://github.com/tina4stack/scoop-bucket
scoop install tina4

# Chocolatey (admin shell)
choco install tina4

# winget
winget install Tina4Stack.Tina4
```

```bash
# Homebrew
brew install tina4stack/tap/tina4

# Debian / Ubuntu - baseline .deb (download then install)
ver=3.8.77
curl -fsSLO https://github.com/tina4stack/tina4/releases/download/v$ver/tina4_${ver}-1_amd64.deb
sudo apt install ./tina4_${ver}-1_amd64.deb    # arm64: tina4_${ver}-1_arm64.deb

# cargo (from source, any OS)
cargo install tina4
```

## One-time setup (per channel)

All accounts use **info@tina4.com** as the publisher identity. For the exact
click-by-click on creating each token/secret, see **[`TOKENS.md`](TOKENS.md)**.

### Scoop
1. Create a public repo `tina4stack/scoop-bucket`.
2. Create a PAT (or fine-grained token) with write access to it; save it as the
   repo secret `SCOOP_BUCKET_TOKEN` on `tina4stack/tina4`.
3. The next release pushes `bucket/tina4.json`. Users add the bucket (above) once.

### Homebrew
1. Create a public repo `tina4stack/homebrew-tap` (the `homebrew-` prefix is what
   lets `brew` resolve the short tap name `tina4stack/tap`).
2. Add a PAT with write access as the secret `HOMEBREW_TAP_TOKEN`.
3. The next release pushes `Formula/tina4.rb`. `brew install tina4stack/tap/tina4`.

### Chocolatey
1. Create an account on https://community.chocolatey.org using info@tina4.com and
   verify the `tina4` package id (first push registers it; it goes through
   moderation the first time).
2. Get the API key (Account → API Keys); save it as the secret `CHOCO_API_KEY`.
3. The next release runs `choco pack` + `choco push`. First push waits for
   moderation; later versions publish automatically once the package is trusted.

### winget
1. The FIRST submission of a new package must be created once by hand, because
   `wingetcreate update` needs an existing entry to update:
   ```powershell
   winget install wingetcreate      # or: iwr https://aka.ms/wingetcreate/latest -OutFile wingetcreate.exe
   wingetcreate new https://github.com/tina4stack/tina4/releases/download/v3.8.77/tina4-windows-amd64.exe
   ```
   Answer the prompts using the values in `packaging/winget/` (identifier
   `Tina4Stack.Tina4`, portable, command alias `tina4`), then let it submit the
   PR to `microsoft/winget-pkgs`. The committed manifests here are the reference
   for those answers.
2. Create a classic PAT with `public_repo` scope (it forks winget-pkgs and opens
   PRs); save it as the secret `WINGET_TOKEN`.
3. Every release after the first runs `wingetcreate update` automatically.

### .deb (baseline)
Nothing. The amd64 + arm64 `.deb`s are built and attached to each release by
`release.yml`. `sudo apt install ./tina4_<ver>-1_amd64.deb`.

### apt.tina4.com (`apt-get install tina4`) — LIVE
A self-hosted, GPG-signed reprepro repository on the `tina4.com` box gives true
`apt-get install tina4` + upgrade-by-name:

```bash
curl -fsSL https://apt.tina4.com/tina4.asc | sudo gpg --dearmor -o /usr/share/keyrings/tina4.gpg
echo "deb [signed-by=/usr/share/keyrings/tina4.gpg] https://apt.tina4.com stable main" | sudo tee /etc/apt/sources.list.d/tina4.list
sudo apt-get update && sudo apt-get install tina4
```

Per release, the maintainer runs `apt-publish.sh <ver>` on the server, which
pulls the release `.debs` and `reprepro includedeb`s them. The signing key lives
only on the server (never in CI). Full details: [`apt/README.md`](apt/README.md).

An `.rpm` + dnf/yum repo (Fedora/RHEL) is the mirror move if demand appears.

## Repository secrets summary

| Secret               | Used by            | What it is |
|----------------------|--------------------|------------|
| `CRATES_IO_TOKEN`    | `release.yml`      | crates.io publish (already set) |
| `SCOOP_BUCKET_TOKEN` | `release-published`| write access to `tina4stack/scoop-bucket` |
| `HOMEBREW_TAP_TOKEN` | `release-published`| write access to `tina4stack/homebrew-tina4` |
| `CHOCO_API_KEY`      | `release-published`| chocolatey.org API key |
| `WINGET_TOKEN`       | `release-published`| PAT (`public_repo`) for winget-pkgs PRs |

The `render` job also commits the bumped manifests back to `main` using the
built-in `GITHUB_TOKEN`. If `main` is protected against direct pushes, either
allow the Actions bot to push or point that step at a PAT - otherwise the render
job's commit-back step fails (the external publishes still run).

## Verifying a change to this pipeline

- `bash scripts/render-manifests.sh <ver> <SHA256SUMS>` then `git diff` - the
  render must touch only version/URL/hash lines.
- `winget validate --manifest packaging/winget`
- `choco pack packaging/chocolatey/tina4.nuspec` - must produce a `.nupkg`
- `python -c "import json; json.load(open('packaging/scoop/tina4.json'))"`
- `.deb`: on a Debian/Ubuntu host with Rust + cargo-deb,
  `cargo build --release && cargo deb` then `sudo dpkg -i` the result,
  `/usr/bin/tina4 --version`, `sudo dpkg -r tina4`.
