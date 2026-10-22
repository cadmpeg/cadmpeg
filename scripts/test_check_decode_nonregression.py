#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Fixed #219 acceptance controls for the independent JSON comparator."""

import copy
import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest


SCRIPT = Path(__file__).with_name('check-decode-nonregression.py')
SPEC = importlib.util.spec_from_file_location('decode_nonregression', SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)
SCHEMA = checker.SchemaPaths(
    variant_tags=frozenset({('features', '#', 'definition', 'kind')}),
    dimensions=frozenset({('features', '#', 'definition', 'length')}),
)


def document(records=None, native=None):
    return {'model': {'features': records or []}, 'native': native or {}}


class FixedAcceptanceControls(unittest.TestCase):
    def test_key_dropped_from_one_of_two_records_fails(self):
        before = document([{'id': 'a', 'values': [1]}, {'id': 'b', 'values': [2]}])
        head = copy.deepcopy(before)
        head['model']['features'][0]['values'].append(3)
        del head['model']['features'][1]['values']
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertTrue(any(f.code == 'neutral-leaf' and f.path[0] == 'b' for f in failures))

    def test_value_set_to_null_fails(self):
        before = document([{'id': 'a', 'value': 7}])
        head = document([{'id': 'a', 'value': None}])
        self.assertTrue(checker.compare_decodes(before, head, SCHEMA))

    def test_offsetting_typed_native_swaps_fail(self):
        before = document([
            {'id': 'a', 'definition': {'kind': 'extrude'}},
            {'id': 'b', 'definition': {'kind': 'Native'}},
        ])
        head = document([
            {'id': 'a', 'definition': {'kind': 'Native'}},
            {'id': 'b', 'definition': {'kind': 'extrude'}},
        ])
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertTrue(any(f.code == 'neutral-tag' and f.path[0] == 'a' for f in failures))

    def test_added_key_passes(self):
        before = document([{'id': 'a', 'value': 7}])
        head = document([{'id': 'a', 'value': 7, 'new': 8}])
        self.assertEqual(checker.compare_decodes(before, head, SCHEMA), [])

    def test_appendix_b_key_removed_with_layout_value_passes(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'offset': 9, 'value': 7}]}})
        head = document(native={'test': {'rows': [{'id': 'r', 'value': 7}]}})
        proof = checker.Evidence(
            native_types={('test', 'rows', 'r'): 'Example'},
            layout_moves=(checker.LayoutMove('test', 'rows', 'Example', ('offset',), 'offset'),),
            fidelity={'record_layout': {'r': {'pointers': {'offset': 9}}}},
        )
        self.assertEqual(checker.compare_decodes(before, head, SCHEMA, proof), [])

    def test_unlisted_integer_field_removed_fails(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'integer': 9}]}})
        head = document(native={'test': {'rows': [{'id': 'r'}]}})
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertTrue(any(f.code == 'native-record' and f.path[-1] == 'integer' for f in failures))

    def test_fixture_deleted_fails(self):
        before = {'test/fixtures/one.bin': document()}
        failures = checker.compare_fixture_sets(before, {}, SCHEMA)
        self.assertEqual([f.code for f in failures], ['fixture-missing'])

    def test_non_active_configuration_override_emptied_fails(self):
        before = {'model': {'configurations': [
            {'id': 'default', 'active': True, 'parameter_overrides': {}},
            {'id': 'other', 'active': False, 'parameter_overrides': {'p': 12}},
        ]}}
        head = copy.deepcopy(before)
        head['model']['configurations'][1]['parameter_overrides'] = {}
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertTrue(any(f.code == 'resolver-evidence-required' and f.path[0] == 'other' for f in failures))

    def test_moved_layout_value_not_stored_fails(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'offset': 9}]}})
        head = document(native={'test': {'rows': [{'id': 'r'}]}})
        proof = checker.Evidence(
            native_types={('test', 'rows', 'r'): 'Example'},
            layout_moves=(checker.LayoutMove('test', 'rows', 'Example', ('offset',), 'offset'),),
        )
        failures = checker.compare_decodes(before, head, SCHEMA, proof)
        self.assertTrue(any(f.code == 'layout-value-preservation' for f in failures))


class ComparatorBoundaryControls(unittest.TestCase):
    def test_dimension_wrapper_is_transparent_only_at_its_typed_path(self):
        before = document([{'id': 'a', 'definition': {'kind': 'extrude', 'length': 7.0}}])
        head = document([{'id': 'a', 'definition': {'kind': 'extrude', 'length': {'kind': 'literal', 'value': 7.0}}}])
        self.assertEqual(checker.compare_decodes(before, head, SCHEMA), [])
        leaves, _ = checker.record_facts(head['model']['features'][0], ('features', '#'), SCHEMA)
        self.assertEqual(leaves[('features', '#', 'definition', 'length')], 1)
        self.assertNotIn(('features', '#', 'definition', 'length', 'kind'), leaves)
        foreign_path = checker.SchemaPaths(frozenset(), frozenset())
        leaves, _ = checker.record_facts(head['model']['features'][0], ('features', '#'), foreign_path)
        self.assertIn(('features', '#', 'definition', 'length', 'kind'), leaves)

    def test_layout_rule_cannot_remove_a_same_named_key_at_a_different_path(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'offset': 9, 'nested': {'offset': 4}}]}})
        head = document(native={'test': {'rows': [{'id': 'r', 'nested': {}}]}})
        proof = checker.Evidence(
            native_types={('test', 'rows', 'r'): 'Example'},
            layout_moves=(checker.LayoutMove('test', 'rows', 'Example', ('offset',), 'offset'),),
            fidelity={'record_layout': {'r': {'pointers': {'offset': 9}}}},
        )
        failures = checker.compare_decodes(before, head, SCHEMA, proof)
        self.assertTrue(any(f.code == 'native-record' and f.path[-2:] == ('nested', 'offset') for f in failures))

    def test_layout_rule_does_not_apply_to_another_type(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'offset': 9}]}})
        head = document(native={'test': {'rows': [{'id': 'r'}]}})
        proof = checker.Evidence(
            native_types={('test', 'rows', 'r'): 'Other'},
            layout_moves=(checker.LayoutMove('test', 'rows', 'Example', ('offset',), 'offset'),),
            fidelity={'record_layout': {'r': {'pointers': {'offset': 9}}}},
        )
        self.assertTrue(any(f.code == 'native-record' for f in checker.compare_decodes(before, head, SCHEMA, proof)))

    def test_wrong_layout_value_is_not_a_preservation_witness(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'offset': 9}]}})
        head = document(native={'test': {'rows': [{'id': 'r'}]}})
        proof = checker.Evidence(
            native_types={('test', 'rows', 'r'): 'Example'},
            layout_moves=(checker.LayoutMove('test', 'rows', 'Example', ('offset',), 'offset'),),
            fidelity={'record_layout': {'r': {'pointers': {'offset': 8}}}},
        )
        self.assertTrue(any(f.code == 'layout-value-preservation' for f in checker.compare_decodes(before, head, SCHEMA, proof)))

    def test_native_added_key_passes(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'value': 9}]}})
        head = document(native={'test': {'rows': [{'id': 'r', 'value': 9, 'new': 8}]}})
        self.assertEqual(checker.compare_decodes(before, head, SCHEMA), [])

    def test_failure_key_is_not_exempt_in_a_different_native_arena(self):
        before = document(native={'inventor': {'other': [{'id': 'r', 'detail': 9}]}})
        head = document(native={'inventor': {'other': [{'id': 'r'}]}})
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertEqual([f.code for f in failures], ['native-record'])

    def test_native_identity_change_cannot_be_offset_by_an_added_record(self):
        before = document(native={'test': {'rows': [{'id': 'r', 'value': 1}]}})
        head = document(native={'test': {'rows': [{'id': 'new', 'value': 1}, {'id': 'extra', 'value': 2}]}})
        self.assertTrue(any(f.code == 'native-identity' for f in checker.compare_decodes(before, head, SCHEMA)))

    def test_native_fractional_tolerance_does_not_tolerate_integer_change(self):
        self.assertTrue(checker.values_agree({'value': 10.0}, {'value': 10.0 * (1 + 1e-13), 'added': 1}))
        self.assertFalse(checker.values_agree({'value': 10.0}, {'value': 10.0 * (1 + 1e-9)}))
        self.assertFalse(checker.values_agree({'value': 10}, {'value': 11}))
        self.assertFalse(checker.values_agree({'value': 10}, {'value': 10.0}))

    def test_unknowns_do_not_disappear_without_reads_coverage(self):
        before = document(native={'test': {'unknowns': [{'id': 'u', 'data': 'abcd'}]}})
        head = document(native={'test': {'unknowns': []}})
        failures = checker.compare_decodes(before, head, SCHEMA)
        self.assertTrue(any(f.code == 'rule-b-evidence-required' for f in failures))

    def test_damaged_move_preserves_the_fixture_base_row(self):
        before = {'test/fixtures/one.bin': document()}
        head = {'test/fixtures/damaged/one.bin': document()}
        self.assertEqual(checker.compare_fixture_sets(before, head, SCHEMA), [])

    def test_script_fails_loudly_without_authoritative_adapters(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn('NOT operational', result.stderr)
        self.assertIn('Appendix B', result.stderr)


if __name__ == '__main__':
    unittest.main()
