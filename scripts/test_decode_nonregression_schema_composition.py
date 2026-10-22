#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Draft 2020-12 reference/sibling validation with bounded object traversal."""

import copy
import unittest

from decode_nonregression_schema import GeneratedArena, GeneratedSchemaError


def query(schema, defs):
    return {'status': 'ok', 'refusal': None, 'payload': {
        'arena': 'model.curves', 'element': 'Curve', 'schema': schema, 'defs': defs,
    }}


def object_schema(properties, required=None):
    return {'type': 'object', 'properties': properties,
            'required': list(properties) if required is None else required}


def nurbs_schema():
    payload = object_schema({'degree': {'type': 'integer', 'minimum': 1},
                             'knots': {'type': 'array', 'items': {'type': 'number'}}})
    branch = {'$ref': '#/$defs/Payload', 'type': 'object',
              'properties': {'kind': {'type': 'string', 'const': 'nurbs'}}, 'required': ['kind']}
    return query({'oneOf': [branch, object_schema({'kind': {'const': 'line', 'type': 'string'}})]},
                 {'Payload': payload})


class ReferenceSiblingCompositionControls(unittest.TestCase):
    def test_actual_tagged_payload_pattern_keeps_reference_fields_and_sibling_tag(self):
        value = nurbs_schema()
        original = copy.deepcopy(value)
        paths = GeneratedArena(value).paths([{'kind': 'nurbs', 'degree': 2, 'knots': [0.0, 1.0]}])
        self.assertEqual(paths.variant_tags, frozenset({('curves', '#', 'kind')}))
        self.assertEqual(value, original)

    def test_reference_and_sibling_constraints_both_refuse_invalid_values(self):
        arena = GeneratedArena(nurbs_schema())
        for value in [
            {'kind': 'nurbs', 'knots': [0.0, 1.0]},
            {'kind': 'nurbs', 'degree': 0, 'knots': [0.0, 1.0]},
            {'kind': 'wrong', 'degree': 2, 'knots': [0.0, 1.0]},
            {'degree': 2, 'knots': [0.0, 1.0]},
        ]:
            with self.subTest(value=value), self.assertRaises(GeneratedSchemaError):
                arena.paths([value])

    def test_duplicate_property_constraints_are_intersections_not_overwrites(self):
        value = nurbs_schema()
        value['payload']['defs']['Payload']['properties']['kind'] = {
            'type': 'string', 'enum': ['nurbs', 'other'],
        }
        arena = GeneratedArena(value)
        self.assertEqual(arena.paths([{'kind': 'nurbs', 'degree': 1, 'knots': []}]).variant_tags,
                         frozenset({('curves', '#', 'kind')}))
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([{'kind': 'other', 'degree': 1, 'knots': []}])
        value['payload']['defs']['Payload']['properties']['kind']['enum'] = ['other']
        with self.assertRaises(GeneratedSchemaError):
            GeneratedArena(value).paths([{'kind': 'nurbs', 'degree': 1, 'knots': []}])

    def test_closed_reference_is_not_opened_by_a_sibling_property_declaration(self):
        value = nurbs_schema()
        value['payload']['defs']['Payload']['additionalProperties'] = False
        with self.assertRaises(GeneratedSchemaError):
            GeneratedArena(value).paths([{'kind': 'nurbs', 'degree': 1, 'knots': []}])

    def test_nested_ref_chain_and_recursive_array_preserve_finite_nested_tag_paths(self):
        defs = {'Payload': object_schema({'children': {
            'type': 'array', 'items': {'$ref': '#/$defs/Tagged'},
        }}), 'Tagged': {'$ref': '#/$defs/Payload', 'type': 'object',
                       'properties': {'kind': {'type': 'string', 'const': 'node'}}, 'required': ['kind']}}
        schema = {'oneOf': [{'$ref': '#/$defs/Tagged'}]}
        paths = GeneratedArena(query(schema, defs)).paths([
            {'kind': 'node', 'children': [{'kind': 'node', 'children': []}]},
        ])
        # The outer oneOf declares a variant; the child is a typed constant
        # object, not a separate enum declaration.
        self.assertEqual(paths.variant_tags, frozenset({('curves', '#', 'kind')}))

    def test_scalar_sibling_assertion_is_still_enforced_by_original_validator(self):
        schema = {'$ref': '#/$defs/Value', 'minimum': 3}
        arena = GeneratedArena(query(schema, {'Value': {'type': 'integer'}}))
        self.assertFalse(arena.paths([3]).variant_tags)
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([2])

    def test_unsupported_structural_intersections_and_reference_cycles_stay_explicit(self):
        for schema, defs in [
            ({'$ref': '#/$defs/Values', 'items': {'minimum': 1}},
             {'Values': {'type': 'array', 'items': {'type': 'integer'}}}),
            ({'$ref': '#/$defs/Cycle', 'properties': {'kind': {'const': 'x'}}},
             {'Cycle': {'$ref': '#/$defs/Cycle'}}),
        ]:
            with self.subTest(schema=schema), self.assertRaises(GeneratedSchemaError):
                GeneratedArena(query(schema, defs))


if __name__ == '__main__':
    unittest.main()
