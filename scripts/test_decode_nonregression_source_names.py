#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual source-name witness protocol controls, independent of real builds."""
import copy
import unittest

from decode_nonregression_source_names import SourceNameError, verify_source_names
from test_decode_nonregression_collect import comparator


def obj(properties, required=None):
    return {'type': 'object', 'additionalProperties': False, 'properties': properties,
            'required': list(properties) if required is None else required}


def identity(value):
    return {'schema_id': value, 'rust_type': value, 'schema_name': value.rsplit('::', 1)[-1]}


def inputs():
    string = {'type': 'string'}
    target = obj({'kind': {'const': 'curve'}, 'id': string})
    assoc = {'format': {'const': 'step'}, 'object_id': string, 'instance_path': {'type': 'array', 'items': string},
             'geometry_role': {'type': 'string'}}
    old_assoc = obj(dict(assoc, name=string, visible={'type': 'boolean'}), ['format', 'object_id'])
    new_assoc = obj(assoc, ['format', 'object_id'])
    curve = lambda source: obj({'id': string, 'source_object': source})
    binding = obj({'id': string, 'target': target, 'appearance': string, 'source_entity_id': string})
    appearance = obj({'id': string})
    attribute = obj({'id': string, 'target': target, 'name': string, 'values': {'type': 'array',
                     'items': obj({'kind': {'const': 'string'}, 'value': string})}})
    def query(arena, element, schema):
        return {'status': 'ok', 'refusal': None, 'payload': {'arena': 'model.' + arena, 'element': element,
                'schema': schema, 'defs': {}, 'type_metadata': {'references': {}}}}
    bq = {'curves': query('curves', 'Curve', curve(old_assoc)),
          'appearance_bindings': query('appearance_bindings', 'AppearanceBinding', binding),
          'appearances': query('appearances', 'Appearance', appearance)}
    hq = dict(bq, curves=query('curves', 'Curve', curve(new_assoc)),
              attributes=query('attributes', 'SourceAttribute', attribute))
    source = {'format': 'step', 'object_id': '#49', 'name': 'two segments'}
    bound = {'id': 'binding', 'target': {'kind': 'curve', 'id': 'curve'}, 'appearance': 'appearance', 'source_entity_id': '#49'}
    asset = {'id': 'appearance'}
    base = {'ir_version': '7', 'model': {'curves': [{'id': 'curve', 'source_object': source}],
            'appearance_bindings': [bound], 'appearances': [asset]}}
    head = copy.deepcopy(base)
    head['ir_version'] = '8'
    del head['model']['curves'][0]['source_object']['name']
    head['model']['attributes'] = [{'id': 'attribute', 'target': {'kind': 'curve', 'id': 'curve'},
                                  'name': 'name', 'values': [{'kind': 'string', 'value': 'two segments'}]}]
    report = {'source_name_protocol': 'baseline-styled-step-curve-name-v1', 'build': 'baseline',
              'ir_version': '7', 'requested': ['curve'], 'rows': [{
                'record_id': 'curve', 'arena': 'curves', 'owner': identity('cadmpeg_ir::geometry::Curve'),
                'association_owner': identity('cadmpeg_ir::provenance::SourceObjectAssociation'),
                'binding_owner': identity('cadmpeg_ir::appearance::AppearanceBinding'),
                'value_path': ['source_object', 'name'], 'value': 'two segments',
                'source_object': copy.deepcopy(source), 'binding': copy.deepcopy(bound), 'appearance': copy.deepcopy(asset)}]}
    return report, base, head, bq, hq


class SourceNameTests(unittest.TestCase):
    def test_value_and_complete_existing_binding_have_one_exact_obligation(self):
        args = inputs()
        result, = verify_source_names(*args)
        self.assertEqual((result.record_id, result.value_path, result.value), ('curve', ('source_object', 'name'), 'two segments'))
        self.assertEqual((result.attribute_id, result.binding_id, result.appearance_id), ('attribute', 'binding', 'appearance'))

    def test_changed_or_wrongly_typed_value_never_passes(self):
        for value in [{'kind': 'string', 'value': 'different'}, {'kind': 'string', 'value': True}, {'kind': 'integer', 'value': 1}]:
            report, base, head, bq, hq = inputs()
            head['model']['attributes'][0]['values'] = [value]
            with self.assertRaises(ValueError): verify_source_names(report, base, head, bq, hq)

    def test_owner_and_instance_binding_are_not_replaced_by_equal_name(self):
        for field, changed in [('object_id', '#50'), ('instance_path', ['another-instance']), ('geometry_role', 'support')]:
            report, base, head, bq, hq = inputs()
            head['model']['curves'][0]['source_object'][field] = changed
            with self.assertRaisesRegex(SourceNameError, 'binding changed'):
                verify_source_names(report, base, head, bq, hq)

    def test_equal_name_on_another_target_or_home_is_not_a_witness(self):
        for changed in ['target', 'home', 'duplicate']:
            report, base, head, bq, hq = inputs()
            attribute = head['model']['attributes'][0]
            if changed == 'target': attribute['target']['id'] = 'other-curve'
            if changed == 'home': attribute['name'] = 'label'
            if changed == 'duplicate':
                other = copy.deepcopy(attribute); other['id'] = 'duplicate'; head['model']['attributes'].append(other)
            with self.assertRaisesRegex(SourceNameError, 'absent or ambiguous'):
                verify_source_names(report, base, head, bq, hq)

    def test_native_owner_target_and_referent_binding_drift_fail(self):
        for field, value in [('source_entity_id', '#50'), ('target', {'kind': 'curve', 'id': 'other'}), ('appearance', 'other')]:
            report, base, head, bq, hq = inputs()
            head['model']['appearance_bindings'][0][field] = value
            with self.assertRaisesRegex(SourceNameError, 'binding changed'):
                verify_source_names(report, base, head, bq, hq)

    def test_missing_referent_and_ambiguous_binding_fail(self):
        report, base, head, bq, hq = inputs()
        head['model']['appearances'] = []
        with self.assertRaisesRegex(SourceNameError, 'referent is absent'): verify_source_names(report, base, head, bq, hq)
        report, base, head, bq, hq = inputs()
        duplicate = copy.deepcopy(head['model']['appearance_bindings'][0]); duplicate['id'] = 'other'
        head['model']['appearance_bindings'].append(duplicate)
        with self.assertRaisesRegex(SourceNameError, 'ambiguous'): verify_source_names(report, base, head, bq, hq)

    def test_unknown_fields_stay_rejected_by_actual_generated_owner_schema(self):
        report, base, head, bq, hq = inputs()
        head['model']['attributes'][0]['unknown'] = 'two segments'
        with self.assertRaisesRegex(ValueError, 'schema/value mismatch'): verify_source_names(report, base, head, bq, hq)
        report, base, head, bq, hq = inputs()
        head['model']['curves'][0]['source_object']['unknown'] = '#49'
        with self.assertRaisesRegex(ValueError, 'schema/value mismatch'): verify_source_names(report, base, head, bq, hq)

    def test_original_field_and_genuine_typed_owner_cannot_be_forged(self):
        report, base, head, bq, hq = inputs()
        report['rows'][0]['value'] = 'other'
        with self.assertRaisesRegex(SourceNameError, 'actual source field'): verify_source_names(report, base, head, bq, hq)
        report, base, head, bq, hq = inputs()
        report['rows'][0]['owner']['rust_type'] = 'other::Curve'
        with self.assertRaisesRegex(SourceNameError, 'typed owner'): verify_source_names(report, base, head, bq, hq)

    def test_typed_f32_colour_spelling_is_not_a_name_or_binding_change(self):
        import struct
        report, base, head, bq, hq = inputs()
        colour = obj({'r': {'type': 'number', 'minimum': 0, 'maximum': 1}})
        for query in [bq['appearances'], hq['appearances']]:
            query['payload']['schema']['properties']['base_color'] = colour
        base['model']['appearances'][0]['base_color'] = {'r': 0.2}
        head['model']['appearances'][0]['base_color'] = {'r': 0.2}
        # Color channels are actual Rust f32 fields. to_value widens this value;
        # the direct JSON serializer writes its shortest roundtripping f32 text.
        admitted = struct.unpack('f', struct.pack('f', 0.2))[0]
        self.assertNotEqual(admitted, 0.2)
        report['rows'][0]['appearance']['base_color'] = {'r': admitted}
        result, = verify_source_names(report, base, head, bq, hq)
        self.assertEqual(result.value, 'two segments')
        head['model']['appearance_bindings'][0]['appearance'] = 'another-appearance'
        with self.assertRaisesRegex(SourceNameError, 'binding changed'):
            verify_source_names(report, base, head, bq, hq)

    def test_original_appearance_unknown_fields_remain_strict(self):
        report, base, head, bq, hq = inputs()
        base['model']['appearances'][0]['unknown'] = 'two segments'
        with self.assertRaisesRegex(ValueError, 'schema/value mismatch'):
            verify_source_names(report, base, head, bq, hq)

    def test_collector_consumes_proof_and_discharges_only_record_name(self):
        from decode_nonregression_collect import compare_collected
        report, base, head, bq, hq = inputs()
        base['model']['curves'][0]['source_object']['visible'] = False
        report['rows'][0]['source_object']['visible'] = False
        class Producer:
            def __init__(self, queries): self.queries = queries
            def schema(self, arena, helper): return self.queries[arena]
        class Helper:
            def run(self, arguments, document):
                self.arguments, self.document = arguments, document
                return report
        helper = Helper()
        findings = compare_collected(comparator, base, head, Producer(bq), Producer(hq), helper)
        self.assertEqual(helper.arguments, ['--source-names', 'curve'])
        self.assertIs(helper.document, base)
        obligations = [finding.path for finding in findings if finding.code == 'resolver-evidence-required']
        self.assertEqual(obligations, [('curve', 'curves', '#', 'source_object', 'visible')])
        self.assertEqual(findings[-1].code, 'coverage-incomplete')


if __name__ == '__main__':
    unittest.main()
