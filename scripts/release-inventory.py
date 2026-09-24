#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Generate SPDX 2.3 and notices from the locked, all-platform Cargo graph.

Uses only Python's standard library and Cargo. This is an auditable inventory,
not legal approval of any licence. Unknown/missing declarations and notices fail.
"""
import argparse
import hashlib
import json
import re
import subprocess
import shutil
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Recognized SPDX identifiers in the locked graph, not an approved-licence policy.
KNOWN_LICENSES = set("MIT Apache-2.0 Unicode-3.0 ISC Unlicense Zlib BSD-3-Clause BSD-2-Clause MPL-2.0 CC0-1.0 LGPL-2.1-or-later BSL-1.0 CDLA-Permissive-2.0 LLVM-exception".split())
NOTICE_NAMES = re.compile(r'^(licen[cs]e|copying|notice|copyright)(?:$|[.-])', re.I)


def generate(output):
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    lock = {}
    for block in (ROOT / 'Cargo.lock').read_text().split('[[package]]')[1:]:
        fields = dict(re.findall(r'^(name|version|checksum|source) = "([^"]+)"$', block, re.M))
        lock[(fields['name'], fields['version'])] = fields
    fallback_dir = ROOT / 'scripts/third-party-licenses'
    fallbacks = json.loads((fallback_dir / 'sources.json').read_text())
    packages = []
    ids = {}
    notices = ['Tina4 CLI third-party notices',
               'All-platform locked dependency inventory (includes build/development dependencies).',
               'Licence declarations are upstream statements, not legal approval.']
    licence_inventory = []
    for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
        name, version = package['name'], package['version']
        declared = package.get('license')
        if not declared or declared in ('NONE', 'NOASSERTION'):
            raise ValueError(f'Missing licence declaration: {name}@{version}')
        # Cargo historically used slash as the OR separator.
        declared = declared.replace('/', ' OR ')
        tokens = re.findall(r'[A-Za-z0-9.-]+', declared)
        unknown = set(tokens) - KNOWN_LICENSES - {'AND', 'OR', 'WITH'}
        if unknown:
            raise ValueError(f'Unrecognized SPDX identifiers for {name}: {sorted(unknown)}')
        ident = 'SPDXRef-' + re.sub(r'[^A-Za-z0-9.-]', '-', name + '-' + version)
        if ident in ids.values():
            raise ValueError(f'Duplicate SPDX identifier: {ident}')
        ids[package['id']] = ident
        entry = {'SPDXID': ident, 'name': name, 'versionInfo': version,
                 'downloadLocation': 'NOASSERTION', 'filesAnalyzed': False,
                 'licenseDeclared': declared, 'licenseConcluded': 'NOASSERTION',
                 'copyrightText': 'NOASSERTION'}
        if package['source']:
            locked = lock[(name, version)]
            if not package['source'].startswith('registry+') or not locked.get('checksum'):
                raise ValueError(f'Unsupported source or absent locked checksum: {name}@{version}')
            entry['downloadLocation'] = f'https://crates.io/api/v1/crates/{name}/{version}/download'
            entry['checksums'] = [{'algorithm': 'SHA256', 'checksumValue': locked['checksum']}]
            entry['externalRefs'] = [{'referenceCategory': 'PACKAGE-MANAGER', 'referenceType': 'purl',
                                      'referenceLocator': f'pkg:cargo/{name}@{version}'}]
            source_dir = Path(package['manifest_path']).parent
            files = sorted(p for p in source_dir.rglob('*') if p.is_file() and NOTICE_NAMES.match(p.name))
            texts = [f'{p.relative_to(source_dir)}\n{p.read_text(errors="strict")}' for p in files]
            if not texts:
                fallback = fallbacks.get(name + '@' + version)
                if not fallback:
                    raise ValueError(f'No licence/notice text: {name}@{version}')
                data = (fallback_dir / fallback['file']).read_bytes()
                if hashlib.sha256(data).hexdigest() != fallback['sha256']:
                    raise ValueError(f'Changed fallback notice: {name}@{version}')
                texts = [data.decode()]
            notices.extend(['\n' + '=' * 72, f'{name} {version}', f'Licence: {declared}',
                            'Authors: ' + ', '.join(package.get('authors', [])),
                            'Repository: ' + (package.get('repository') or 'not declared'), *texts])
        packages.append(entry)
        licence_inventory.append({'name': name, 'version': version, 'licenseDeclared': declared,
                                  'approvalStatus': 'not-assessed'})
    root_id = metadata['resolve']['root']
    relationships = [{'spdxElementId': 'SPDXRef-DOCUMENT', 'relationshipType': 'DESCRIBES',
                      'relatedSpdxElement': ids[root_id]}]
    for node in metadata['resolve']['nodes']:
        for dependency in node['dependencies']:
            relationships.append({'spdxElementId': ids[node['id']], 'relationshipType': 'DEPENDS_ON',
                                  'relatedSpdxElement': ids[dependency]})
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    version = next(p['version'] for p in metadata['packages'] if p['id'] == root_id)
    lock_digest = hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest()
    document = {'spdxVersion': 'SPDX-2.3', 'dataLicense': 'CC0-1.0', 'SPDXID': 'SPDXRef-DOCUMENT',
                'name': 'tina4-' + version,
                'documentNamespace': f'https://tina4.com/spdx/cli/{version}/{commit}-{lock_digest}',
                'creationInfo': {'creators': ['Tool: tina4-release-inventory'],
                                 'created': datetime.fromisoformat(subprocess.check_output(['git', 'show', '-s', '--format=%cI', 'HEAD'], cwd=ROOT, text=True).strip()).astimezone(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')},
                'comment': 'All-platform Cargo.lock inventory; includes build/dev dependencies. No legal approval claimed.',
                'packages': packages, 'relationships': relationships}
    output.mkdir(parents=True, exist_ok=True)
    for notice in ['LICENSE', 'NOTICE', 'COMMERCIAL-LICENSE.md']:
        shutil.copyfile(ROOT / notice, output / notice)
    (output / 'tina4.spdx.json').write_text(json.dumps(document, indent=2) + '\n')
    (output / 'THIRD-PARTY-NOTICES.txt').write_text('\n\n'.join(notices) + '\n')
    (output / 'LICENSE-INVENTORY.json').write_text(json.dumps(licence_inventory, indent=2) + '\n')
    print(f'Inventoried {len(packages)} packages; all have declarations and third-party notices.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    generate(parser.parse_args().output)
