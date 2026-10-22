#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compiler identity bridge controls; synthetic JSON is not producer evidence."""

import copy
import unittest

from decode_nonregression_schema import GeneratedSchemaError
from decode_nonregression_schema_bindings import attach_external, from_query, generated_arena


DIMENSION = 'cadmpeg_ir::features::dimensions::Dimension<cadmpeg_ir::scalar::Length>'
OWNER = 'cadmpeg_ir::sketches::SketchConstraintDefinition'
PARAMETER = 'cadmpeg_ir::features::ParameterId'


def variant(kind, value):
    return {'type': 'object', 'properties': {'kind': {'const': kind, 'type': 'string'},
                                           'value': value},
            'required': ['kind', 'value'], 'additionalProperties': False}


def binding(role, identity):
    return {'role': role, 'schema_id': identity, 'rust_type': identity}


def query():
    dimension = {'oneOf': [variant('literal', {'type': 'number'}),
                           variant('parameter', {'type': 'string'})]}
    return {'status': 'ok', 'refusal': None, 'payload': {
        'arena': 'model.features', 'element': 'Record',
        'schema': {'type': 'object', 'properties': {'length': {'$ref': '#/$defs/Uninformative7'}},
                   'required': ['length'], 'additionalProperties': False},
        'defs': {'Uninformative7': dimension},
        'type_metadata': {'references': {'#/$defs/Uninformative7': [binding('dimension', DIMENSION)]}},
    }}


def external_for(value):
    payload = value['payload']
    return {'schema': {'properties': {'model': {'$ref': '#/$defs/Model'}}, '$defs': {
        **payload['defs'], 'Record': payload['schema'],
        'Model': {'properties': {'features': {'items': {'$ref': '#/$defs/Record'}}}},
    }}, 'type_metadata': copy.deepcopy(payload['type_metadata'])}


class CompilerIdentityBridgeControls(unittest.TestCase):
    def test_compiler_identity_binds_a_ref_without_guessing_its_name_or_shape(self):
        value = query()
        types = from_query(value)
        self.assertEqual(types.bindings.dimensions, frozenset({'Uninformative7'}))
        paths = generated_arena(value).paths([{'length': {'kind': 'literal', 'value': 3.0}}])
        self.assertEqual(paths.dimensions, frozenset({('features', '#', 'length')}))
        self.assertFalse(paths.variant_tags)

    def test_missing_baseline_metadata_is_not_guessed_from_dimension_shaped_values(self):
        value = query()
        del value['payload']['type_metadata']
        with self.assertRaises(GeneratedSchemaError):
            generated_arena(value)

    def test_conflicting_roles_rust_types_and_unresolved_refs_refuse(self):
        for patch in [
            {'role': 'dimension', 'schema_id': 'other::Dimension', 'rust_type': 'other::Dimension'},
            binding('d10_owner', DIMENSION),
            binding('parameter_id', 'other::ParameterId'),
            binding('unrecognized', DIMENSION),
        ]:
            value = query()
            value['payload']['type_metadata']['references']['#/$defs/Uninformative7'] = [patch]
            with self.subTest(patch=patch), self.assertRaises(GeneratedSchemaError):
                from_query(value)
        value = query()
        value['payload']['type_metadata']['references']['#/$defs/Missing'] = [binding('dimension', DIMENSION)]
        with self.assertRaises(GeneratedSchemaError):
            from_query(value)

    def test_shared_refs_and_duplicate_identity_refs_fail_closed(self):
        value = query()
        references = value['payload']['type_metadata']['references']
        references['#/$defs/Uninformative7'].append(binding('dimension', DIMENSION))
        with self.assertRaises(GeneratedSchemaError):
            from_query(value)
        value = query()
        value['payload']['defs']['Another'] = copy.deepcopy(value['payload']['defs']['Uninformative7'])
        value['payload']['type_metadata']['references']['#/$defs/Another'] = [binding('dimension', DIMENSION)]
        with self.assertRaises(GeneratedSchemaError):
            from_query(value)

    def test_authentic_d10_owner_and_target_are_not_a_field_pair_exemption(self):
        value = query()
        payload = value['payload']
        payload['schema'] = {'type': 'object', 'properties': {
            'parameter': {'anyOf': [{'$ref': '#/$defs/ParameterId'}, {'type': 'null'}]},
        }, 'additionalProperties': False}
        payload['defs']['ParameterId'] = {'type': 'string'}
        payload['type_metadata']['references']['#/$defs/Record'] = [binding('d10_owner', OWNER)]
        payload['type_metadata']['references']['#/$defs/ParameterId'] = [binding('parameter_id', PARAMETER)]
        types = from_query(value)
        self.assertEqual(types.d10_owners, frozenset({'Record'}))
        self.assertEqual(types.parameter_targets, frozenset({'ParameterId'}))
        self.assertEqual(types.bindings.companions, ())

    def test_external_baseline_root_requires_exact_element_and_defs_agreement(self):
        value = query()
        external = external_for(value)
        del value['payload']['type_metadata']
        paired = attach_external(value, external)
        self.assertEqual(from_query(paired).bindings.dimensions, frozenset({'Uninformative7'}))
        self.assertNotIn('type_metadata', value['payload'])
        for mutate in [
            lambda row: row['payload']['schema']['properties'].update(extra={'type': 'string'}),
            lambda row: row['payload']['defs']['Uninformative7'].update(description='different'),
            lambda row: row['payload'].update(element='Another'),
            lambda row: row['payload']['defs'].update(unreached={'type': 'string'}),
        ]:
            wrong = copy.deepcopy(value)
            mutate(wrong)
            with self.subTest(mutate=mutate), self.assertRaises(GeneratedSchemaError):
                attach_external(wrong, external)

    def test_external_metadata_cannot_replace_current_compiler_bindings(self):
        value = query()
        with self.assertRaises(GeneratedSchemaError):
            attach_external(value, external_for(value))

    def test_schema_id_and_rust_type_may_have_legitimate_different_generic_spellings(self):
        value = query()
        entry = value['payload']['type_metadata']['references']['#/$defs/Uninformative7'][0]
        entry['schema_id'] = 'cadmpeg_ir::features::dimensions::Dimension<NonZeroU32>'
        entry['rust_type'] = 'cadmpeg_ir::features::dimensions::Dimension<core::num::nonzero::NonZero<u32>>'
        self.assertEqual(from_query(value).bindings.dimensions, frozenset({'Uninformative7'}))


if __name__ == '__main__':
    unittest.main()
