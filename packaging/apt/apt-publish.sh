#!/usr/bin/env bash
# Publish released tina4 .deb(s) into the apt.tina4.com repository.
#
# Run ON the apt server (tina4.com) as the tina4 user, once per release:
#   ssh andre@tina4.com 'sudo -u tina4 /home/tina4/domains/apt.tina4.com/reprepro/apt-publish.sh 3.8.78'
#
# It pulls the official, CI-built .debs attached to the GitHub release and adds
# them to the reprepro repo (which signs the indices with the on-server GPG key).
# The signing key never leaves this box - matching the manual Docker-image model.
set -euo pipefail
VER="${1:?usage: apt-publish.sh <version>  e.g. 3.8.78}"
BASE=/home/tina4/domains/apt.tina4.com/reprepro
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

got=0
for arch in amd64 arm64; do
  f="tina4_${VER}-1_${arch}.deb"
  url="https://github.com/tina4stack/tina4/releases/download/v${VER}/${f}"
  if curl -fsSL "$url" -o "$TMP/$f"; then
    reprepro -b "$BASE" includedeb stable "$TMP/$f"
    echo "included $f"; got=$((got+1))
  else
    echo "skip $arch - no asset at $url"
  fi
done
[ "$got" -gt 0 ] || { echo "ERROR: no .deb published for $VER"; exit 1; }
echo "--- stable now contains ---"
reprepro -b "$BASE" list stable
