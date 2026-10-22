#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Presence/contents controls and strict actual arena-adoption enforcement."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('check-removed-names.py')
SPEC = importlib.util.spec_from_file_location('removed_name_check', SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


class RegistryPresenceTests(unittest.TestCase):
    def test_every_required_name_is_present_and_no_other_item_exists(self):
        values = checker.parse_registry((checker.ROOT / checker.REGISTRY).read_text())
        self.assertEqual(set(values.values()), checker.REQUIRED_NAMES)

    def test_docs_are_metadata_not_extra_rust_items(self):
        text = (checker.ROOT / checker.REGISTRY).read_text()
        self.assertEqual(checker.parse_registry('// metadata\n' + text), checker.parse_registry(text))

    def test_missing_name_is_not_accepted(self):
        text = (checker.ROOT / checker.REGISTRY).read_text()
        text = '\n'.join(line for line in text.splitlines() if 'pub const ACTIVE_BREP:' not in line)
        with self.assertRaisesRegex(checker.RegistryError, 'missing='):
            checker.parse_registry(text)

    def test_other_item_is_not_accepted(self):
        text = (checker.ROOT / checker.REGISTRY).read_text() + '\nfn helper() {}\n'
        with self.assertRaisesRegex(checker.RegistryError, 'only pub const'):
            checker.parse_registry(text)

    def test_duplicate_value_is_not_accepted(self):
        text = (checker.ROOT / checker.REGISTRY).read_text() + '\npub const DUPLICATE: &str = "active_brep";\n'
        with self.assertRaisesRegex(checker.RegistryError, 'duplicate'):
            checker.parse_registry(text)


class AbsenceMechanismTests(unittest.TestCase):
    def test_actual_native_and_appendix_cells_are_both_checked(self):
        with tempfile.TemporaryDirectory() as temporary:
            table = Path(temporary) / 'parts.tsv'
            table.write_text('codec\tlevel\tarenas\nexample\tpart\tkept, design_intent_failures\n')
            failures = checker.absence_findings({'design_intent_failures'}, {'example': ['design_intent_failures']}, table)
        self.assertEqual(len(failures), 2)
        self.assertTrue(any('native registry' in message for message in failures))
        self.assertTrue(any('Appendix C' in message for message in failures))

    def test_missing_appendix_fails_instead_of_skipping(self):
        with tempfile.TemporaryDirectory() as temporary:
            failures = checker.absence_findings(set(), {}, Path(temporary) / 'absent.tsv')
        self.assertTrue(any('Appendix C is required' in message for message in failures))


class ActualRegistryAdoptionTests(unittest.TestCase):
    def test_all_production_registries_and_appendix_c_are_free_of_removed_names(self):
        self.assertEqual(checker.check(checker.ROOT), [])


if __name__ == '__main__':
    unittest.main()
