# apt.tina4.com — hosted APT repository

`apt-get install tina4` on Debian/Ubuntu, with signed metadata and
upgrade-by-name. Self-hosted with **reprepro** on the `tina4.com` box (the same
Virtualmin server that serves the docs), signed by a GPG key that **never leaves
that server** — the same manual, credential-stays-on-the-host model used for the
framework Docker images.

## Install (end users)

```bash
curl -fsSL https://apt.tina4.com/tina4.asc | sudo gpg --dearmor -o /usr/share/keyrings/tina4.gpg
echo "deb [signed-by=/usr/share/keyrings/tina4.gpg] https://apt.tina4.com stable main" | sudo tee /etc/apt/sources.list.d/tina4.list
sudo apt-get update
sudo apt-get install tina4
```

`apt-get upgrade` then keeps `tina4` current. Works on amd64 and arm64.

## How it's wired

```
GitHub Release  ──(cargo deb in release.yml)──>  tina4_<ver>-1_{amd64,arm64}.deb
                                                          │
        maintainer runs, once per release:                │  curl
   ssh andre@tina4.com 'sudo -u tina4 .../apt-publish.sh <ver>'
                                                          ▼
   reprepro includedeb stable  ->  signs indices with the on-server GPG key
                                                          │
                    outdir = the Apache docroot            ▼
        /home/tina4/domains/apt.tina4.com/public_html/{dists,pool}/  +  tina4.asc
                                                          │  https (Apache)
                                                          ▼
                                    apt-get on any Debian/Ubuntu host
```

Layout on the server:

| Path | Role | Web-served? |
|------|------|-------------|
| `/home/tina4/domains/apt.tina4.com/public_html/` | Apache docroot (`dists/`, `pool/`, `tina4.asc`) | **yes** |
| `/home/tina4/domains/apt.tina4.com/reprepro/` | reprepro base — `conf/`, `db/`, `apt-publish.sh` | no |
| `…/reprepro/.gnupg/` | the signing key (mode 700) | no |

`reprepro`'s `outdir`/`gnupghome` (see [`conf/options`](conf/options)) put the
public tree in the docroot while `conf/`, `db/`, and the key stay outside it — so
there are no Apache deny-rules to get wrong.

## Signing key

- Identity: `Tina4 CLI Repository <info@tina4.com>`
- Fingerprint: `BD0EB84909E29BC5E87E5D08 39783C6CB3084433`
- Passphraseless (reprepro signs unattended), protected by the `700` gnupg homedir.
- Public key published at <https://apt.tina4.com/tina4.asc>.
- **Back up** `…/reprepro/.gnupg` off-box; losing it means re-keying every client.

## Publishing a new release

The `.deb`s are built and attached to the GitHub release automatically
(`release.yml`). After a release is published, run once:

```bash
ssh andre@tina4.com 'sudo -u tina4 /home/tina4/domains/apt.tina4.com/reprepro/apt-publish.sh 3.8.78'
```

[`apt-publish.sh`](apt-publish.sh) pulls the official amd64 + arm64 `.debs` from
the release and `reprepro includedeb`s them (skipping an arch whose asset is
missing). This is deliberately manual, matching the Docker-image publish — the
signing key is a trust primitive and is never handed to CI.

## One-time server setup (for disaster recovery)

Reproduces what's already live. On the server (`tina4.com`), the docroot is a
Virtualmin virtual server for `apt.tina4.com`:

```bash
sudo apt-get install -y reprepro gnupg
# as the domain user, so ownership is correct:
sudo -u tina4 bash -s <<'SETUP'
set -e
BASE=/home/tina4/domains/apt.tina4.com/reprepro
PUB=/home/tina4/domains/apt.tina4.com/public_html
export GNUPGHOME="$BASE/.gnupg"
mkdir -p "$BASE/conf" "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
cat > "$BASE/keyparams" <<'K'
%no-protection
Key-Type: RSA
Key-Length: 4096
Key-Usage: sign
Name-Real: Tina4 CLI Repository
Name-Email: info@tina4.com
Expire-Date: 0
%commit
K
gpg --batch --gen-key "$BASE/keyparams"; rm -f "$BASE/keyparams"
FPR=$(gpg --list-keys --with-colons info@tina4.com | awk -F: '/^fpr:/{print $10; exit}')
gpg --armor --export "$FPR" > "$PUB/tina4.asc"
# then copy conf/distributions (with SignWith: $FPR), conf/options, and
# apt-publish.sh from this packaging/apt/ directory into $BASE.
SETUP
```

The Apache vhost just serves the docroot over HTTPS (Virtualmin already
provisioned the cert). Directory indexes are off and don't need to be on — apt
fetches fixed paths (`dists/…/InRelease`, `pool/…/*.deb`).

## Verifying

```bash
curl -fsSI https://apt.tina4.com/dists/stable/InRelease            # 200, signed (SHA512)
curl -fsSL https://apt.tina4.com/dists/stable/InRelease | head -2  # BEGIN PGP SIGNED MESSAGE
# full loop (throwaway host): add the key+source above, apt-get update,
# apt-get install tina4, tina4 --version, apt-get remove tina4.
```
