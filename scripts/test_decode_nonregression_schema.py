#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Generated-schema classification controls; no corpus or resolver claim."""

import copy
import unittest

from decode_nonregression_schema import (
    BuildPaths, Companion, GeneratedArena, GeneratedSchemaError, TypeBindings,
)


def query(schema, defs=None, element='Record'):
    return {'status': 'ok', 'refusal': None, 'payload': {
        'arena': 'model.features', 'schema': schema,
        'defs': {'Record': schema, **(defs or {})}, 'element': element,
    }}


def object_schema(properties, required=None):
    return {'type': 'object', 'properties': properties,
            'required': list(properties) if required is None else required,
            'additionalProperties': False}


def variant(tag, **properties):
    return object_schema({'kind': {'type': 'string', 'const': tag}, **properties})


class GeneratedSchemaControls(unittest.TestCase):
    def test_ordinary_kind_field_is_not_a_variant_discriminant(self):
        arena = GeneratedArena(query(object_schema({'kind': {'type': 'string'}})))
        paths = arena.paths([{'kind': 'native'}, {'kind': 'extrude'}])
        self.assertEqual(paths.variant_tags, frozenset())

    def test_actual_variant_tags_are_counted_with_nested_arrays_and_refs(self):
        defs = {'Definition': {'oneOf': [variant('extrude'), variant('native')]}}
        schema = object_schema({'grid': {'type': 'array', 'items': {
            'type': 'array', 'items': {'$ref': '#/$defs/Definition'},
        }}})
        paths = GeneratedArena(query(schema, defs)).paths([
            {'grid': [[{'kind': 'extrude'}, {'kind': 'native'}]]},
        ])
        self.assertEqual(paths.variant_tags, frozenset({('features', '#', 'grid', '#', '#', 'kind')}))

    def test_recursive_schema_stops_at_actual_finite_document_boundaries(self):
        node = object_schema({
            'kind': {'type': 'string'},
            'children': {'type': 'array', 'items': {'$ref': '#/$defs/Node'}},
            'definition': {'$ref': '#/$defs/Definition'},
        })
        defs = {'Node': node, 'Definition': {'oneOf': [variant('cut'), variant('pad')]}}
        arena = GeneratedArena(query({'$ref': '#/$defs/Node'}, defs))
        paths = arena.paths([{
            'kind': 'label', 'definition': {'kind': 'cut'},
            'children': [{'kind': 'other-label', 'definition': {'kind': 'pad'}, 'children': []}],
        }])
        self.assertEqual(paths.variant_tags, frozenset({
            ('features', '#', 'definition', 'kind'),
            ('features', '#', 'children', '#', 'definition', 'kind'),
        }))

    def test_flattened_union_reference_preserves_terminal_and_nested_tags(self):
        defs = {'Nested': {'oneOf': [variant('a'), variant('b')]}}
        arena = GeneratedArena(query({'oneOf': [
            {'$ref': '#/$defs/Nested'}, variant('c'),
        ]}, defs))
        self.assertEqual(arena.paths([{'kind': 'c'}]).variant_tags,
                         frozenset({('features', '#', 'kind')}))

    def test_dimension_passthrough_requires_exact_type_identity_not_same_shape(self):
        dimension = {'oneOf': [
            variant('literal', value={'type': 'number'}),
            variant('parameter', value={'$ref': '#/$defs/ParameterId'}),
        ]}
        defs = {'Dimension_Length': dimension, 'Imitation': copy.deepcopy(dimension),
                'ParameterId': {'type': 'string'}}
        schema = object_schema({'length': {'$ref': '#/$defs/Dimension_Length'},
                                'other': {'$ref': '#/$defs/Imitation'}})
        arena = GeneratedArena(query(schema, defs), TypeBindings(frozenset({'Dimension_Length'})))
        paths = arena.paths([{'length': {'kind': 'literal', 'value': 4.0},
                              'other': {'kind': 'literal', 'value': 4.0}}])
        self.assertEqual(paths.dimensions, frozenset({('features', '#', 'length')}))
        self.assertEqual(paths.variant_tags, frozenset({('features', '#', 'other', 'kind')}))

    def test_d10_companion_requires_owner_target_and_variant_binding(self):
        parameter = {'anyOf': [{'$ref': '#/$defs/OffsetParameter'}, {'type': 'null'}]}
        defs = {'Definition': {'oneOf': [variant('offset', parameter=parameter),
                                        variant('native', parameter={'type': 'string'})]},
                'OffsetParameter': object_schema({'id': {'$ref': '#/$defs/ParameterId'},
                                                  'negated': {'type': 'boolean'}}),
                'ParameterId': {'type': 'string'}}
        bindings = TypeBindings(companions=(Companion(
            'Definition', 'parameter', 'OffsetParameter', ('kind', 'offset'),
        ),))
        arena = GeneratedArena(query({'$ref': '#/$defs/Definition'}, defs), bindings)
        paths = arena.paths([{'kind': 'offset', 'parameter': {'id': 'p', 'negated': False}}])
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([{'kind': 'offset', 'parameter': {'id': 'p', 'negated': False}},
                         {'kind': 'native', 'parameter': 'authored'}])
        self.assertEqual(paths.d10_companions, frozenset({('features', '#', 'parameter')}))
        unrelated = GeneratedArena(query(object_schema({'parameter': {'type': 'string'}})))
        self.assertEqual(unrelated.paths([{'parameter': 'p'}]).d10_companions, frozenset())

    def test_base_and_head_keep_distinct_type_identities_and_paths(self):
        base = GeneratedArena(query(object_schema({'kind': {'type': 'string'}})))
        head = GeneratedArena(query({'oneOf': [variant('pad'), variant('cut')]}))
        pair = BuildPaths(base.paths([{'kind': 'cut'}]), head.paths([{'kind': 'cut'}]))
        self.assertFalse(pair.base.variant_tags)
        self.assertEqual(pair.head.variant_tags, frozenset({('features', '#', 'kind')}))

    def test_malformed_unresolved_external_and_nonprogressing_schema_fail_closed(self):
        for schema, defs in [
            ({'$ref': '#/$defs/Missing'}, {}),
            ({'$ref': 'https://example.invalid/schema'}, {}),
            ({'type': 'impossible'}, {}),
            ({'oneOf': []}, {}),
            ({'$ref': '#/$defs/Cycle'}, {'Cycle': {'$ref': '#/$defs/Cycle'}}),
        ]:
            with self.subTest(schema=schema), self.assertRaises(GeneratedSchemaError):
                GeneratedArena(query(schema, defs)).paths([{}])

    def test_invalid_value_and_mismatched_d10_target_fail_closed(self):
        with self.assertRaises(GeneratedSchemaError):
            GeneratedArena(query({'oneOf': [variant('pad'), variant('cut')]})).paths([{'kind': 'third'}])
        defs = {'ParameterId': {'type': 'string'}}
        arena = GeneratedArena(query(object_schema({'parameter': {'type': 'string'}}), defs),
                               TypeBindings(companions=(Companion('Record', 'parameter', 'ParameterId'),)))
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([{'parameter': 'label'}])

    def test_real_tag_counts_and_dimension_leaves_feed_existing_comparator(self):
        import importlib.util
        from pathlib import Path
        import sys
        spec = importlib.util.spec_from_file_location(
            'schema_comparator_control', Path(__file__).with_name('check-decode-nonregression.py'),
        )
        checker = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = checker
        spec.loader.exec_module(checker)
        defs = {'Dimension_Length': {'oneOf': [
            variant('literal', value={'type': 'number'}),
            variant('parameter', value={'type': 'string'}),
        ]}, 'Definition': {'oneOf': [variant('cut'), variant('native')]}}
        schema = object_schema({'id': {'type': 'string'}, 'kind': {'type': 'string'},
                                'length': {'$ref': '#/$defs/Dimension_Length'},
                                'operations': {'type': 'array', 'items': {'$ref': '#/$defs/Definition'}}})
        record = {'id': 'r', 'kind': 'ordinary', 'length': {'kind': 'literal', 'value': 3.0},
                  'operations': [{'kind': 'cut'}, {'kind': 'cut'}, {'kind': 'native'}]}
        paths = GeneratedArena(query(schema, defs), TypeBindings(frozenset({'Dimension_Length'}))).paths([record])
        leaves, tags = checker.record_facts(record, ('features', '#'), paths)
        self.assertEqual(dict(tags), {(('features', '#', 'operations', '#', 'kind'), 'cut'): 2})
        self.assertEqual(leaves[('features', '#', 'length')], 1)
        self.assertNotIn(('features', '#', 'length', 'kind'), leaves)
        self.assertEqual(leaves[('features', '#', 'kind')], 1)

    def test_dynamic_and_conditional_schemas_fail_before_any_external_resolution(self):
        for schema in [
            {'$dynamicRef': 'https://example.invalid/schema'},
            {'$id': 'https://example.invalid/schema', '$ref': '#/$defs/X'},
            {'if': {'type': 'string'}, 'then': {'type': 'string'}},
        ]:
            with self.subTest(schema=schema), self.assertRaises(GeneratedSchemaError):
                GeneratedArena(query(schema))

    def test_mixed_tag_and_ordinary_role_at_one_collapsed_path_fails_closed(self):
        defs = {'Tagged': {'oneOf': [variant('a'), variant('b')]},
                'Plain': object_schema({'kind': {'type': 'string'}, 'ordinary': {'type': 'boolean'}})}
        schema = {'oneOf': [{'$ref': '#/$defs/Tagged'}, {'$ref': '#/$defs/Plain'}]}
        arena = GeneratedArena(query(schema, defs))
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([{'kind': 'a'}, {'kind': 'label', 'ordinary': True}])
        self.assertEqual(arena.paths([{'kind': 'a'}]).variant_tags,
                         frozenset({('features', '#', 'kind')}))
        self.assertFalse(arena.paths([{'kind': 'label', 'ordinary': True}]).variant_tags)

    def test_generated_unit_variants_are_tags_and_nonfinite_values_are_refused(self):
        schema = object_schema({'display': {'oneOf': [
            {'type': 'string', 'const': 'radius'}, {'type': 'string', 'const': 'diameter'},
        ]}, 'value': {'type': 'number'}})
        arena = GeneratedArena(query(schema))
        self.assertEqual(arena.paths([{'display': 'radius', 'value': 1.0}]).variant_tags,
                         frozenset({('features', '#', 'display')}))
        with self.assertRaises(GeneratedSchemaError):
            arena.paths([{'display': 'radius', 'value': float('nan')}])

    def test_nullable_generated_unit_variant_keeps_its_tag_path(self):
        schema = object_schema({'display': {'anyOf': [
            {'$ref': '#/$defs/DimensionDisplay'}, {'type': 'null'},
        ]}})
        defs = {'DimensionDisplay': {'oneOf': [
            {'type': 'string', 'const': 'radius'}, {'type': 'string', 'const': 'diameter'},
        ]}}
        paths = GeneratedArena(query(schema, defs)).paths([{'display': 'radius'}, {'display': None}])
        self.assertEqual(paths.variant_tags, frozenset({('features', '#', 'display')}))

    def test_structured_untyped_hole_is_not_silently_traversed(self):
        with self.assertRaises(GeneratedSchemaError):
            GeneratedArena(query(object_schema({'content': True}))).paths([{'content': {'kind': 'pad'}}])


if __name__ == '__main__':
    unittest.main()
