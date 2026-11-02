#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the fuzz gate selects the targets that use a codec."""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from scripts.ci_fuzz import command, target_sources


class FuzzGateTests(unittest.TestCase):
    def test_nx_selects_harness_and_seed_generator(self):
        check = command(['nx'])
        self.assertIn('nx_geometry_points', check)
        self.assertIn('generate_comprehensive_seeds', check)
        self.assertNotIn('catia_b5', check)
        self.assertIn('--lib', check)

    def test_shared_changes_keep_full_gate(self):
        self.assertIn('--all-targets', command([]))
        self.assertNotIn('--no-default-features', command([]))

    def test_codec_references_in_harness_modules_are_selected(self):
        self.assertIn('fcstd_xml', command(['freecad']))

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

    def test_unknown_feature_uses_full_gate(self):
        self.assertIn('--all-targets', command(['not-a-codec']))

    def test_unresolved_module_uses_full_gate(self):
        with patch('scripts.ci_fuzz.target_sources', side_effect=ValueError):
            self.assertIn('--all-targets', command(['inventor']))


if __name__ == '__main__':
    unittest.main()
