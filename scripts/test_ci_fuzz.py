#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the fuzz gate selects the harnesses and generators that use a codec."""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from scripts.ci_fuzz import command, target_sources


class FuzzGateTests(unittest.TestCase):
    def test_nx_selects_harness_and_seed_tests(self):
        check = command('check', ['nx'])
        self.assertIn('nx_geometry_points', check)
        self.assertNotIn('catia_b5', check)
        test = command('test', ['nx'])
        self.assertIn('generate_comprehensive_seeds', test)
        self.assertNotIn('nx_geometry_points', test)
        self.assertIn('--lib', test)

    def test_shared_changes_keep_full_gate(self):
        self.assertIn('--all-targets', command('check', []))
        self.assertNotIn('--no-default-features', command('test', []))

    def test_codec_references_in_harness_modules_are_selected(self):
        self.assertIn('fcstd_xml', command('check', ['freecad']))

    def test_harness_follows_nested_module_directories(self):
        with TemporaryDirectory() as folder:
            root = Path(folder) / 'fuzz_targets'
            (root / 'helper').mkdir(parents=True)
            (root / 'harness.rs').write_text('mod helper;\n')
            (root / 'helper.rs').write_text('pub(crate) mod child;\n')
            (root / 'helper' / 'child.rs').write_text('use cadmpeg_codec_iges::IgesCodec;\n')
            self.assertIn('cadmpeg_codec_iges', target_sources(root / 'harness.rs'))

    def test_inline_module_with_external_children_falls_back(self):
        with TemporaryDirectory() as folder:
            source = Path(folder) / 'harness.rs'
            source.write_text('mod outer {\n    mod child;\n}\n')
            with self.assertRaises(ValueError):
                target_sources(source)

    def test_rejects_unknown_feature(self):
        with self.assertRaises(ValueError):
            command('check', ['not-a-codec'])

    def test_unresolved_module_uses_full_gate(self):
        with patch('scripts.ci_fuzz.target_sources', side_effect=ValueError):
            self.assertIn('--all-targets', command('check', ['inventor']))


if __name__ == '__main__':
    unittest.main()
