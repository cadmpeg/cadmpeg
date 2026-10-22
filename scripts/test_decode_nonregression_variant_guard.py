#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Variant guards use actual tagged union paths rather than authored labels."""
import copy
import unittest

from decode_nonregression_variant_guard import uncertain_records, compare_uncertain_records, compare_uncertain_totals
from decode_nonregression_schema import GeneratedSchemaError


def object_schema(properties):
    return {'type': 'object', 'properties': properties, 'required': list(properties), 'additionalProperties': False}


def query():
    variants = {'oneOf': [object_schema({'kind': {'const': kind, 'type': 'string'},
                                       'label': {'type': 'string'}}) for kind in ['known', 'unknown', 'unresolved', 'other']]}
    schema = object_schema({'id': {'type': 'string'}, 'kind': {'type': 'string'},
                            'children': {'type': 'array', 'items': {'$ref': '#/$defs/Variant'}}})
    return {'status': 'ok', 'refusal': None, 'payload': {'arena': 'model.features', 'element': 'Record',
        'schema': schema, 'defs': {'Variant': variants}, 'type_metadata': {'references': {}}}}


def row(identity='r', variants=('known',), label='known', kind='ordinary'):
    return {'id': identity, 'kind': kind, 'children': [{'kind': variant, 'label': label} for variant in variants]}


class VariantGuardTests(unittest.TestCase):
    def test_real_tag_increase_fails_without_neutral_count_loss(self):
        for variant in ['unknown', 'unresolved', 'other']:
            findings = compare_uncertain_records('features', [row()], [row(variants=(variant,))], query(), query())
            self.assertEqual([finding.code for finding in findings], ['uncertain-variant-increase'])

    def test_ordinary_kind_and_authored_text_are_not_variant_proof(self):
        for text in ['unknown', 'unresolved', 'other', 'Unknown', 'Other']:
            self.assertEqual(uncertain_records([row(label=text, kind=text)], query()), set())

    def test_several_uncertain_nodes_in_one_record_count_once(self):
        self.assertEqual(uncertain_records([row(variants=('unknown', 'unresolved', 'other'))], query()), {'r'})
        self.assertEqual(compare_uncertain_records('features', [row(variants=('unknown',))],
                         [row(variants=('unknown', 'other'))], query(), query()), [])

    def test_additional_uncertain_record_fails_even_when_tag_occurrences_fall(self):
        before = [row(variants=('unknown', 'unknown', 'unknown'))]
        after = [row('r', ('unknown',)), row('s', ('other',))]
        self.assertEqual(len(compare_uncertain_records('features', before, after, query(), query())), 1)

    def test_head_only_arena_and_unchanged_or_decreased_counts(self):
        self.assertEqual(len(compare_uncertain_records('features', [], [row(variants=('unknown',))], None, query())), 1)
        self.assertEqual(compare_uncertain_records('features', [], [row()], None, query()), [])
        self.assertEqual(compare_uncertain_records('features', [row(variants=('unknown',))], [row()], query(), query()), [])

    def test_golden_total_allows_arena_rehoming_without_hiding_an_increase(self):
        self.assertEqual(compare_uncertain_totals({('features', 'r')}, {('drawings', 'r')}), [])
        self.assertEqual(len(compare_uncertain_totals({('features', 'r')},
                             {('drawings', 'r'), ('curves', 's')})), 1)

    def test_unknown_fields_and_unresolved_schema_refuse(self):
        invalid = row(); invalid['children'][0]['extra'] = True
        with self.assertRaises(GeneratedSchemaError): uncertain_records([invalid], query())
        invalid_schema = query(); invalid_schema['payload']['defs'].clear()
        with self.assertRaises(GeneratedSchemaError): uncertain_records([row()], invalid_schema)

    def test_missing_metadata_wrong_arena_and_duplicate_ids_refuse(self):
        invalid = query(); del invalid['payload']['type_metadata']
        with self.assertRaises(GeneratedSchemaError): uncertain_records([row()], invalid)
        with self.assertRaises(GeneratedSchemaError): compare_uncertain_records('curves', [], [], None, query())
        with self.assertRaises(GeneratedSchemaError): compare_uncertain_records('features', [row()], [], None, query())
        with self.assertRaises(GeneratedSchemaError): uncertain_records([row(), row()], query())

    def test_recursive_union_arrays_are_finite_and_use_actual_tags(self):
        q = query()
        variants = q['payload']['defs']['Variant']['oneOf']
        for variant in variants:
            variant['properties']['nested'] = {'type': 'array', 'items': {'$ref': '#/$defs/Variant'}}
            variant['required'].append('nested')
        record = row(); record['children'][0]['nested'] = [{'kind': 'other', 'label': 'text', 'nested': []}]
        self.assertEqual(uncertain_records([record], q), {'r'})


if __name__ == '__main__':
    unittest.main()
