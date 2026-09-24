# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Real-file negative checks for the pre-sign integrity gate; no signing/network."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('verify_release_inputs', ROOT / 'scripts/verify-release-inputs.py')
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class ChecksumGate(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        for name in ['LICENSE', 'NOTICE', 'COMMERCIAL-LICENSE.md', 'tina4.spdx.json', 'THIRD-PARTY-NOTICES.txt', 'LICENSE-INVENTORY.json', 'tina4-windows-amd64.exe']:
            (self.directory / name).write_bytes(('checksum fixture: ' + name).encode())
        self.manifest = self.directory / 'SHA256SUMS'
        self.manifest.write_text(''.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name + '\n'
                                         for p in sorted(self.directory.iterdir())))

    def test_complete_matching_assets(self):
        self.assertEqual(len(verify.verify_checksums(self.directory)), 8)

    def test_tampered_asset_is_rejected(self):
        with (self.directory / 'tina4-windows-amd64.exe').open('ab') as file:
            file.write(b'tamper')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            verify.verify_checksums(self.directory)

    def test_unlisted_asset_is_rejected(self):
        (self.directory / 'unexpected.exe').write_bytes(b'unlisted')
        with self.assertRaisesRegex(ValueError, 'exactly cover'):
            verify.verify_checksums(self.directory)

    def test_missing_notice_is_rejected(self):
        (self.directory / 'THIRD-PARTY-NOTICES.txt').unlink()
        self.manifest.write_text('\n'.join(line for line in self.manifest.read_text().splitlines()
                                           if 'THIRD-PARTY' not in line) + '\n')
        with self.assertRaisesRegex(ValueError, 'missing required'):
            verify.verify_checksums(self.directory)

    def test_duplicate_and_traversal_entries_are_rejected(self):
        original = self.manifest.read_text()
        for extra in [original.splitlines()[0], '0' * 64 + '  ../escape']:
            self.manifest.write_text(original + extra + '\n')
            with self.assertRaisesRegex(ValueError, 'Invalid or duplicate'):
                verify.verify_checksums(self.directory)


if __name__ == '__main__':
    unittest.main()
