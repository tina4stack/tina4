#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Verify draft bytes and tag-bound CI provenance before any local signing."""
import argparse
import hashlib
import re
import subprocess
from pathlib import Path


def verify_checksums(directory):
    manifest = directory / 'SHA256SUMS'
    entries = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r'([a-fA-F0-9]{64}) [ *]([A-Za-z0-9_.+-]+)', line)
        if not match or match[2] in entries:
            raise ValueError('Invalid or duplicate checksum entry')
        entries[match[2]] = match[1].lower()
    actual = {p.name for p in directory.iterdir() if p.is_file() and p.name != 'SHA256SUMS'}
    if set(entries) != actual:
        raise ValueError('Checksum manifest does not exactly cover the downloaded assets')
    required = {'LICENSE', 'NOTICE', 'COMMERCIAL-LICENSE.md', 'tina4.spdx.json', 'THIRD-PARTY-NOTICES.txt', 'LICENSE-INVENTORY.json',
                'tina4-windows-amd64.exe'}
    if not required <= actual:
        raise ValueError('Release is missing required binary, SBOM or licence assets')
    for name, digest in entries.items():
        if hashlib.sha256((directory / name).read_bytes()).hexdigest() != digest:
            raise ValueError('Checksum mismatch: ' + name)
    return [manifest] + [directory / name for name in sorted(entries)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--repo', required=True)
    parser.add_argument('--tag', required=True)
    args = parser.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+', args.tag):
        raise ValueError('Expected a final vMAJOR.MINOR.PATCH release tag')
    for asset in verify_checksums(args.directory):
        print('Verifying tag-bound CI provenance: ' + asset.name, flush=True)
        subprocess.run(['gh', 'attestation', 'verify', str(asset), '--repo', args.repo,
                        '--cert-identity', f'https://github.com/{args.repo}/.github/workflows/release.yml@refs/tags/{args.tag}'],
                       check=True)
    print('Every downloaded asset matches its checksum and CI provenance; signing may proceed.')


if __name__ == '__main__':
    main()
