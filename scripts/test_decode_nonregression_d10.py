#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""D10 pair value/binding controls, independent of corpus enumeration."""

import copy
import unittest

from decode_nonregression_d10 import D10ResolutionError, verify_length_pairs, OWNER, LENGTH, DIMENSION_LENGTH


def obj(properties):
    return {'type': 'object', 'properties': properties, 'required': list(properties), 'additionalProperties': False}


def identity(role, name):
    return {'schema_id': name, 'rust_type': name, 'role': role}


def schema_query(arena, schema, defs, refs):
    return {'status': 'ok', 'refusal': None, 'payload': {
        'arena': 'model.' + arena, 'element': 'Record', 'schema': schema, 'defs': defs,
        'type_metadata': {'references': refs},
    }}


def inputs(variant='distance_loci_value', bound='p'):
    field = 'parameter' if variant == 'distance_loci_value' else 'distance_parameter'
    report = {'resolver_protocol': 'd10-length-pairs-v1', 'build': 'baseline', 'ir_version': '7',
              'requested': ['r'], 'rows': [{
                  'record_id': 'r', 'arena': 'sketch_constraints',
                  'owner': {'schema_id': OWNER, 'rust_type': OWNER}, 'variant': variant,
                  'value_path': ['definition', 'distance'], 'companion_path': ['definition', field],
                  'quantity': {'schema_id': LENGTH, 'rust_type': LENGTH}, 'value': 3.0, 'parameter': bound,
              }]}
    parameter = {'anyOf': [{'type': 'string'}, {'type': 'null'}]}
    variants = lambda distance, companion: {'oneOf': [obj({
        'kind': {'type': 'string', 'const': kind}, 'distance': distance, **extra,
    }) for kind, extra in [('distance_loci_value', {'parameter': parameter} if companion else {}),
                           ('polar_distance', {'distance_parameter': parameter} if companion else {})]]}
    root = obj({'id': {'type': 'string'}, 'definition': {'$ref': '#/$defs/Definition'}})
    base = schema_query('sketch_constraints', root, {'Definition': variants({'type': 'number'}, True)},
                        {'#/$defs/Definition': [identity('d10_owner', OWNER)]})
    dimension = {'oneOf': [obj({'kind': {'type': 'string', 'const': 'literal'}, 'value': {'type': 'number'}}),
                           obj({'kind': {'type': 'string', 'const': 'parameter'}, 'value': {'type': 'string'}})]}
    head = schema_query('sketch_constraints', root, {
        'Definition': variants({'$ref': '#/$defs/Dim'}, False), 'Dim': dimension,
    }, {'#/$defs/Definition': [identity('d10_owner', OWNER)],
        '#/$defs/Dim': [identity('dimension', DIMENSION_LENGTH)]})
    cache = {'anyOf': [{'type': 'null'}, obj({'kind': {'type': 'string'}, 'value': {'type': 'number'}})]}
    parameters = schema_query('parameters', obj({'id': {'type': 'string'}, 'value': cache}), {}, {})
    wrapper = {'kind': 'parameter', 'value': bound} if bound else {'kind': 'literal', 'value': 3.0}
    ir = {'model': {'sketch_constraints': [{'id': 'r', 'definition': {'kind': variant, 'distance': wrapper}}],
                    'parameters': [{'id': bound, 'value': {'kind': 'length', 'value': 3.0}}] if bound else []}}
    return report, ir, base, head, parameters


class D10EffectiveLengthControls(unittest.TestCase):
    def test_actual_pair_protocol_proves_value_and_binding_for_one_exact_record(self):
        proofs = verify_length_pairs(*inputs())
        self.assertEqual(len(proofs), 1)
        self.assertEqual(proofs[0].record_id, 'r')
        self.assertEqual(proofs[0].companion_path, ('definition', 'parameter'))
        self.assertEqual(proofs[0].value, 3.0)
        self.assertEqual(proofs[0].parameter, 'p')

    def test_unbound_literal_and_polar_pair_keep_their_actual_companion_field(self):
        proofs = verify_length_pairs(*inputs('polar_distance', None))
        self.assertIsNone(proofs[0].parameter)
        self.assertEqual(proofs[0].companion_path, ('definition', 'distance_parameter'))

    def test_same_effective_value_cannot_hide_a_changed_parameter_binding(self):
        data = list(inputs())
        data[1]['model']['parameters'][0]['id'] = 'q'
        data[1]['model']['sketch_constraints'][0]['definition']['distance']['value'] = 'q'
        with self.assertRaises(D10ResolutionError):
            verify_length_pairs(*data)

    def test_value_drift_fails_and_existing_numeric_comparison_tolerance_is_preserved(self):
        data = list(inputs())
        data[1]['model']['parameters'][0]['value']['value'] = 4.0
        with self.assertRaises(D10ResolutionError):
            verify_length_pairs(*data)
        data[1]['model']['parameters'][0]['value']['value'] = 3.0 + 1.0e-13
        self.assertEqual(len(verify_length_pairs(*data)), 1)

    def test_missing_unevaluated_wrong_quantity_and_duplicate_parameters_refuse(self):
        for mutation in [
            lambda ir: ir['model'].update(parameters=[]),
            lambda ir: ir['model']['parameters'][0].update(value=None),
            lambda ir: ir['model']['parameters'][0]['value'].update(kind='angle'),
            lambda ir: ir['model']['parameters'].append(copy.deepcopy(ir['model']['parameters'][0])),
        ]:
            data = list(inputs())
            mutation(data[1])
            with self.subTest(mutation=mutation), self.assertRaises(D10ResolutionError):
                verify_length_pairs(*data)

    def test_untyped_imitation_or_missing_compiler_metadata_cannot_supply_dimension_proof(self):
        data = list(inputs())
        del data[3]['payload']['type_metadata']
        with self.assertRaises(D10ResolutionError):
            verify_length_pairs(*data)
        data = list(inputs())
        data[3]['payload']['type_metadata']['references']['#/$defs/Dim'][0]['rust_type'] = 'other::Imitation'
        with self.assertRaises(D10ResolutionError):
            verify_length_pairs(*data)

    def test_unknown_pair_paths_variants_and_incomplete_requested_batches_fail_closed(self):
        for mutation in [
            lambda report: report['rows'][0].update(variant='native'),
            lambda report: report['rows'][0].update(companion_path=['definition', 'unrelated']),
            lambda report: report['rows'][0].update(value_path=['definition', 'other']),
            lambda report: report['requested'].append('missing'),
            lambda report: report.update(requested=[]),
        ]:
            data = list(inputs())
            mutation(data[0])
            with self.subTest(mutation=mutation), self.assertRaises(D10ResolutionError):
                verify_length_pairs(*data)

    def test_nonfinite_boolean_and_unknown_wrapper_values_cannot_resolve(self):
        for wrapper in [{'kind': 'literal', 'value': True}, {'kind': 'literal', 'value': float('nan')},
                        {'kind': 'unknown', 'value': 3.0}]:
            data = list(inputs(bound=None))
            data[1]['model']['sketch_constraints'][0]['definition']['distance'] = wrapper
            with self.subTest(wrapper=wrapper), self.assertRaises(D10ResolutionError):
                verify_length_pairs(*data)


if __name__ == '__main__':
    unittest.main()
