#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
import copy
import unittest

from decode_nonregression_native_links import NativeLinkError, verify_native_links


def inputs():
    document = {'model': {'attributes': [{'id': 'owner', 'name': 'target'}]},
                'native': {'codec': {'objects': [{'id': 'target'}]}}}
    report = {'status': 'ok', 'refusal': None, 'payload': {'protocol': 'cadmpeg-typed-native-links-v1',
        'ir_sha256': 'hash', 'rows': [{'arena': 'attributes', 'rust_type': 'actual::SourceAttribute',
                                    'id': 'owner', 'native_ref': 'target'}]}}
    return report, document, 'hash'


class NativeLinksTests(unittest.TestCase):
    def test_actual_typed_link_names_one_existing_native_target(self):
        findings, witnesses = verify_native_links(*inputs())
        self.assertEqual(findings, [])
        self.assertEqual([(r['arena'], r['id'], r['native_namespace'], r['native_arena']) for r in witnesses],
                         [('attributes', 'owner', 'codec', 'objects')])

    def test_matching_label_does_not_supply_a_native_link(self):
        data = inputs(); data[0]['payload']['rows'][0]['native_ref'] = None
        self.assertEqual(verify_native_links(*data), ([], []))

    def test_missing_and_cross_arena_ambiguous_native_targets_fail(self):
        for duplicate in [False, True]:
            data = inputs()
            if duplicate: data[1]['native']['codec']['other'] = [{'id': 'target'}]
            else: data[1]['native']['codec']['objects'] = []
            findings, witnesses = verify_native_links(*data)
            self.assertEqual([r.code for r in findings], ['native-projection-target'])
            self.assertEqual(witnesses, [])

    def test_byte_binding_and_query_refusal_fail_closed(self):
        for mutate in [lambda x: x.update(status='refused'), lambda x: x['payload'].update(ir_sha256='different'),
                       lambda x: x['payload'].update(protocol='inferred-json')]:
            data = inputs(); mutate(data[0])
            with self.assertRaises(NativeLinkError): verify_native_links(*data)

    def test_missing_duplicate_foreign_and_unknown_owner_rows_fail(self):
        for kind in ['missing', 'duplicate', 'foreign', 'extra']:
            data = inputs(); rows = data[0]['payload']['rows']
            if kind == 'missing': rows.clear()
            if kind == 'duplicate': rows.append(copy.deepcopy(rows[0]))
            if kind == 'foreign': rows[0]['id'] = 'other'
            if kind == 'extra': rows[0]['ignored'] = True
            with self.assertRaises(NativeLinkError): verify_native_links(*data)

    def test_all_unlinked_records_are_still_required_in_inventory(self):
        data = inputs(); data[1]['model']['bodies'] = [{'id': 'body'}]
        with self.assertRaises(NativeLinkError): verify_native_links(*data)
        data[0]['payload']['rows'].append({'arena': 'bodies', 'rust_type': 'actual::Body', 'id': 'body', 'native_ref': None})
        self.assertEqual(len(verify_native_links(*data)[1]), 1)


if __name__ == '__main__':
    unittest.main()
