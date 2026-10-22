#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Historical wire-bridge controls; local reports do not prove real builds."""
import copy
import unittest

from decode_nonregression_wire import verify_wire_rows, WireAdmissionError


def inputs(arena='procedural_surfaces'):
    surface = arena == 'procedural_surfaces'
    owner = 'ProceduralSurface' if surface else 'ProceduralCurve'
    carrier_type = 'SurfaceId' if surface else 'CurveId'
    owner_name, carrier_name = 'cadmpeg_ir::geometry::' + owner, 'cadmpeg_ir::ids::' + carrier_type
    payload = {'id': 'r', 'definition': {'kind': 'exact', 'value': 3.0}, 'optional_bound': None}
    wire = {'id': 'r', 'definition': payload['definition'], 'emitted_link': 'carrier'}
    schema = {'type': 'object', 'required': ['id', 'definition'], 'additionalProperties': False,
              'properties': {'id': {'type': 'string'}, 'optional_bound': {'type': 'null'},
                             'definition': {'type': 'object', 'additionalProperties': False,
                                            'required': ['kind', 'value'],
                                            'properties': {'kind': {'const': 'exact'},
                                                           'value': {'type': 'number', 'minimum': 0}}}}}
    query = {'status': 'ok', 'refusal': None, 'payload': {'arena': 'model.' + arena, 'element': owner, 'schema': schema,
                         'defs': {}, 'type_metadata': {'references': {}}}}
    report = {'wire_protocol': 'baseline-procedural-wire-v1', 'build': 'baseline', 'ir_version': '7',
              'requested': ['r'], 'rows': [{'record_id': 'r', 'arena': arena,
                   'owner': {'schema_id': owner_name, 'rust_type': owner_name, 'schema_name': owner},
                   'carrier_identity': {'schema_id': carrier_name, 'rust_type': carrier_name, 'schema_name': carrier_type},
                   'carrier': 'carrier', 'carrier_path': ['emitted_link'], 'wire_record': wire,
                   'payload_record': payload}]}
    original = {'ir_version': '7', 'model': {arena: [copy.deepcopy(wire)]}}
    return report, original, {arena: query}


class WireAdmissionTests(unittest.TestCase):
    def test_both_actual_public_owner_families_have_record_specific_admission(self):
        for arena in ['procedural_surfaces', 'procedural_curves']:
            report, original, queries = inputs(arena)
            proof = verify_wire_rows(report, original, queries)[arena, 'r']
            self.assertEqual(proof.carrier, 'carrier')
            self.assertEqual(proof.carrier_path, ('emitted_link',))
            self.assertFalse(proof.paths.dimensions)

    def test_all_public_payload_constraints_still_apply(self):
        report, original, queries = inputs()
        report['rows'][0]['payload_record']['definition']['value'] = -1
        original['model']['procedural_surfaces'][0]['definition']['value'] = -1
        with self.assertRaisesRegex(ValueError, 'schema/value mismatch'):
            verify_wire_rows(report, original, queries)
        report, original, queries = inputs()
        report['rows'][0]['payload_record']['unknown'] = True
        with self.assertRaises(ValueError):
            verify_wire_rows(report, original, queries)

    def test_unknown_wire_field_cannot_be_excused_as_carrier(self):
        report, original, queries = inputs()
        report['rows'][0]['wire_record']['unknown'] = 'carrier'
        original['model']['procedural_surfaces'][0]['unknown'] = 'carrier'
        with self.assertRaisesRegex(WireAdmissionError, 'missing or changed'):
            verify_wire_rows(report, original, queries)

    def test_tampered_or_nonroundtripped_source_refuses(self):
        report, original, queries = inputs()
        report['rows'][0]['wire_record']['emitted_link'] = 'other'
        with self.assertRaisesRegex(WireAdmissionError, 'differs from the source'):
            verify_wire_rows(report, original, queries)
        report, original, queries = inputs()
        report['rows'][0]['payload_record']['definition'] = {'kind': 'exact', 'value': 4.0}
        with self.assertRaisesRegex(WireAdmissionError, 'missing or changed'):
            verify_wire_rows(report, original, queries)

    def test_missing_duplicate_and_cross_owner_requests_refuse(self):
        for change in ['missing', 'duplicate', 'owner', 'carrier']:
            report, original, queries = inputs()
            if change == 'missing': report['rows'] = []
            if change == 'duplicate': report['requested'] *= 2
            if change == 'owner': report['rows'][0]['owner']['rust_type'] = 'other::ProceduralSurface'
            if change == 'carrier': report['rows'][0]['carrier_identity']['rust_type'] = 'other::SurfaceId'
            with self.assertRaises(WireAdmissionError):
                verify_wire_rows(report, original, queries)

    def test_no_changed_payload_field_is_hidden_by_carrier_path(self):
        report, original, queries = inputs()
        report['rows'][0]['carrier_path'] = ['definition']
        with self.assertRaisesRegex(WireAdmissionError, 'not uniquely represented'):
            verify_wire_rows(report, original, queries)


if __name__ == '__main__':
    unittest.main()
