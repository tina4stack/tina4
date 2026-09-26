# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Real-run checks for the SPDX SBOM generator; no mocks, no network stubs.

The happy path runs the actual generator over the actual repository lockfile via
Cargo, then reads the emitted SPDX back and proves it names the ``tina4`` package
and its resolved dependencies. The trust-boundary cases feed ``build_document``
real, hand-built graphs and prove it raises the moment the graph is unsound - so a
generator that stopped rejecting a bad graph would turn these red.
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('release_inventory', ROOT / 'scripts/release-inventory.py')
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)

REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'


def graph(root_extra=None, dep_extra=None):
    """A minimal but real two-crate resolved graph: the tina4 root plus one dep."""
    root = {'id': 'root-id', 'name': 'tina4', 'version': '9.9.9', 'license': 'MPL-2.0',
            'source': None, 'manifest_path': '/nonexistent/Cargo.toml',
            'authors': ['Tina4 Stack'], 'repository': 'https://github.com/tina4stack/tina4'}
    dep = {'id': 'dep-id', 'name': 'demo-dep', 'version': '1.2.3', 'license': 'MIT',
           'source': REGISTRY, 'manifest_path': None,
           'authors': ['Some Author'], 'repository': 'https://example.invalid/demo-dep'}
    root.update(root_extra or {})
    dep.update(dep_extra or {})
    metadata = {'packages': [root, dep],
                'resolve': {'root': 'root-id',
                            'nodes': [{'id': 'root-id', 'dependencies': ['dep-id']},
                                      {'id': 'dep-id', 'dependencies': []}]}}
    lock = {('tina4', '9.9.9'): {'name': 'tina4', 'version': '9.9.9'},
            ('demo-dep', '1.2.3'): {'name': 'demo-dep', 'version': '1.2.3', 'checksum': 'a' * 64,
                                    'source': REGISTRY}}
    return metadata, lock


class SbomGeneratorRealRun(unittest.TestCase):
    def test_generator_names_tina4_and_its_dependencies_from_the_real_lockfile(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'dist'
            inventory.generate(output)   # real Cargo, real Cargo.lock, real git HEAD - no doubles
            document = json.loads((output / 'tina4.spdx.json').read_text())

        self.assertEqual(document['spdxVersion'], 'SPDX-2.3')
        self.assertEqual(document['dataLicense'], 'CC0-1.0')
        packages = document['packages']
        self.assertGreater(len(packages), 1, 'SBOM must inventory dependencies, not just the root')

        root = next(p for p in packages if p['name'] == 'tina4')
        self.assertRegex(root['versionInfo'], r'^\d+\.\d+\.\d+')
        describes = [r for r in document['relationships'] if r['relationshipType'] == 'DESCRIBES']
        self.assertEqual([r['relatedSpdxElement'] for r in describes], [root['SPDXID']])
        self.assertTrue(any(r['relationshipType'] == 'DEPENDS_ON' for r in document['relationships']),
                        'a real dependency graph must carry DEPENDS_ON edges')

        # Every relationship endpoint resolves to a declared package (no dangling refs).
        declared = {'SPDXRef-DOCUMENT'} | {p['SPDXID'] for p in packages}
        for relationship in document['relationships']:
            self.assertIn(relationship['spdxElementId'], declared)
            self.assertIn(relationship['relatedSpdxElement'], declared)

        # Every crates.io package carries a cargo purl and a SHA-256 that matches Cargo.lock.
        lock = inventory.parse_lock((ROOT / 'Cargo.lock').read_text())
        registry_packages = [p for p in packages if p['downloadLocation'].startswith('https://crates.io/')]
        self.assertGreater(len(registry_packages), 0)
        for package in registry_packages:
            purl = package['externalRefs'][0]['referenceLocator']
            self.assertEqual(purl, f"pkg:cargo/{package['name']}@{package['versionInfo']}")
            self.assertEqual(package['checksums'][0]['checksumValue'],
                             lock[(package['name'], package['versionInfo'])]['checksum'])

    def test_valid_hand_built_graph_produces_a_resolvable_document(self):
        with tempfile.TemporaryDirectory() as crate:
            (Path(crate) / 'LICENSE').write_text('MIT licence text for demo-dep')
            metadata, lock = graph(dep_extra={'manifest_path': str(Path(crate) / 'Cargo.toml')})
            document, notices, licence_inventory = inventory.build_document(
                metadata, lock, Path(crate), {}, 'c0ffee', '2026-01-01T00:00:00Z', 'd' * 64)

        names = {p['name'] for p in document['packages']}
        self.assertEqual(names, {'tina4', 'demo-dep'})
        self.assertEqual(document['name'], 'tina4-9.9.9')
        dep = next(p for p in document['packages'] if p['name'] == 'demo-dep')
        self.assertEqual(dep['externalRefs'][0]['referenceLocator'], 'pkg:cargo/demo-dep@1.2.3')
        self.assertEqual(dep['checksums'][0]['checksumValue'], 'a' * 64)
        self.assertIn({'spdxElementId': 'SPDXRef-tina4-9.9.9', 'relationshipType': 'DEPENDS_ON',
                       'relatedSpdxElement': 'SPDXRef-demo-dep-1.2.3'}, document['relationships'])
        self.assertTrue(any('demo-dep 1.2.3' in line for line in notices))
        self.assertEqual([e['approvalStatus'] for e in licence_inventory], ['not-assessed', 'not-assessed'])


class SbomGeneratorRejectsUnsoundGraphs(unittest.TestCase):
    """Prove the trust boundary is a gate: break the graph, the generator refuses."""

    def build(self, root_extra=None, dep_extra=None, fallback_dir=None, fallbacks=None):
        metadata, lock = graph(root_extra, dep_extra)
        return inventory.build_document(metadata, lock, fallback_dir or Path('/nonexistent'),
                                        fallbacks or {}, 'c0ffee', '2026-01-01T00:00:00Z', 'd' * 64)

    def test_missing_licence_declaration_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Missing licence declaration'):
            self.build(dep_extra={'license': None})

    def test_unrecognized_spdx_identifier_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Unrecognized SPDX identifiers'):
            self.build(dep_extra={'license': 'Frobnicate-1.0'})

    def test_registry_crate_without_checksum_is_rejected(self):
        metadata, lock = graph()
        del lock[('demo-dep', '1.2.3')]['checksum']
        with self.assertRaisesRegex(ValueError, 'absent locked checksum'):
            inventory.build_document(metadata, lock, Path('/nonexistent'), {},
                                     'c0ffee', '2026-01-01T00:00:00Z', 'd' * 64)

    def test_registry_crate_without_any_notice_text_is_rejected(self):
        with tempfile.TemporaryDirectory() as crate:   # a real, empty crate dir: no LICENSE/NOTICE
            with self.assertRaisesRegex(ValueError, 'No licence/notice text'):
                self.build(dep_extra={'manifest_path': str(Path(crate) / 'Cargo.toml')})

    def test_a_previously_valid_dependency_turning_bad_flips_green_to_red(self):
        # Mutation proof: the same graph that builds cleanly must fail once corrupted.
        with tempfile.TemporaryDirectory() as crate:
            (Path(crate) / 'LICENSE').write_text('MIT licence text for demo-dep')
            self.build(dep_extra={'manifest_path': str(Path(crate) / 'Cargo.toml')})   # green
            with self.assertRaisesRegex(ValueError, 'Unrecognized SPDX identifiers'):
                self.build(dep_extra={'manifest_path': str(Path(crate) / 'Cargo.toml'),
                                      'license': 'Totally-Not-A-Licence'})              # red


if __name__ == '__main__':
    unittest.main()
