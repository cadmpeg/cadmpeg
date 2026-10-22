#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""The actual unknown-count exception needs bytes and successful typed reads."""
import copy
import importlib.util
from pathlib import Path
import unittest

from decode_nonregression_unknown_reads import unknown_read_witnesses, discharge_unknown_findings
from decode_nonregression_retained import RetainedEvidenceError
from test_decode_nonregression_retained import image

spec = importlib.util.spec_from_file_location('unknown_read_comparator', Path(__file__).with_name('check-decode-nonregression.py'))
comparator = importlib.util.module_from_spec(spec)
import sys
sys.modules[spec.name] = comparator
spec.loader.exec_module(comparator)


def layout(start=4, end=8, stream='member'):
    return {'stream': 'member', 'offset': 0, 'byte_len': 12,
            'reads': {stream: [{'start': start, 'end': end}]}, 'pointers': {}, 'payloads': {}}


def inputs():
    base = {'native': {'codec': {'unknowns': [{'id': 'unknown'}]}}}
    head = {'native': {'codec': {'typed': [{'id': 'reader'}]}}}
    sidecar = {'fidelity': {'retained_records': {'unknown': image()}}}
    after = copy.deepcopy(sidecar)
    after['fidelity']['record_layout'] = {'reader': layout()}
    return base, head, sidecar, after


def findings(base, head):
    return comparator.compare_native(base.get('native', {}), head.get('native', {}), comparator.Evidence())


class UnknownReadTests(unittest.TestCase):
    def test_actual_reads_of_surviving_record_discharge_exact_unknown(self):
        args = inputs()
        proof = unknown_read_witnesses(*args)
        self.assertEqual([(p.namespace, p.record_id, p.stream, p.start, p.end, p.readers) for p in proof],
                         [('codec', 'unknown', 'member', 4, 8, ('reader',))])
        self.assertEqual(discharge_unknown_findings(findings(*args[:2]), *args[:2], proof), [])

    def test_adjacent_and_overlapping_readers_cover_exact_extent(self):
        for first_end, second_start in [(6, 6), (7, 5)]:
            args = inputs()
            args[1]['model'] = {'points': [{'id': 'reader2'}]}
            args[3]['fidelity']['record_layout'] = {'reader': layout(4, first_end), 'reader2': layout(second_start, 8)}
            self.assertEqual(unknown_read_witnesses(*args)[0].readers, ('reader', 'reader2'))

    def test_gap_wrong_stream_or_missing_read_has_no_credit(self):
        for candidate in [layout(4, 7), layout(4, 8, 'another'), dict(layout(), reads={})]:
            args = inputs(); args[3]['fidelity']['record_layout']['reader'] = candidate
            self.assertEqual(unknown_read_witnesses(*args), [])
        args = inputs(); args[1]['model'] = {'points': [{'id': 'reader2'}]}
        args[3]['fidelity']['record_layout'] = {'reader': layout(4, 5), 'reader2': layout(6, 8)}
        self.assertEqual(unknown_read_witnesses(*args), [])

    def test_extent_payload_and_pointer_alone_never_count_as_reads(self):
        args = inputs(); candidate = layout(); candidate['reads'] = {}
        candidate['pointers'] = {'start': 4, 'end': 8}; candidate['payloads'] = {'copy': '41424344'}
        args[3]['fidelity']['record_layout']['reader'] = candidate
        self.assertEqual(unknown_read_witnesses(*args), [])

    def test_absent_unknown_or_ambiguous_reader_cannot_supply_credit(self):
        for kind in ['absent', 'unknown', 'ambiguous', 'unknown_collision']:
            args = inputs()
            if kind == 'absent': args[1]['native']['codec']['typed'] = []
            if kind == 'unknown': args[1]['native']['codec'] = {'unknowns': [{'id': 'reader'}]}
            if kind == 'ambiguous': args[1]['model'] = {'points': [{'id': 'reader'}]}
            if kind == 'unknown_collision': args[1]['native']['other'] = {'unknowns': [{'id': 'reader'}]}
            self.assertEqual(unknown_read_witnesses(*args), [])

    def test_same_bytes_without_same_available_source_extent_never_prove_consumption(self):
        for change in ['missing', 'offset', 'stream', 'bytes', 'unnamed', 'empty']:
            args = inputs(); records = args[3]['fidelity']['retained_records']
            if change == 'missing': records.clear()
            if change == 'offset': records['unknown']['offset'] += 1
            if change == 'stream': records['unknown']['stream'] = 'renamed'
            if change == 'bytes': records['unknown'] = image(b'ABCE')
            if change == 'unnamed':
                args[2]['fidelity']['retained_records']['unknown']['stream'] = ''
                records['unknown']['stream'] = ''
            if change == 'empty':
                args[2]['fidelity']['retained_records']['unknown'] = image(b'')
                records['unknown'] = image(b'')
            self.assertEqual(unknown_read_witnesses(*args), [])

    def test_every_base_unknown_must_have_reads_before_an_arena_count_falls(self):
        args = inputs()
        args[0]['native']['codec']['unknowns'].append({'id': 'remaining'})
        args[1]['native']['codec']['unknowns'] = [{'id': 'remaining'}]
        for sidecar in args[2:]: sidecar['fidelity']['retained_records']['remaining'] = image(b'EFGH', offset=8)
        proof = unknown_read_witnesses(*args)
        reduced = discharge_unknown_findings(findings(*args[:2]), *args[:2], proof)
        self.assertEqual([row.code for row in reduced], ['rule-b-evidence-required'])
        args[3]['fidelity']['record_layout']['reader'] = layout(4, 12)
        proof = unknown_read_witnesses(*args)
        self.assertEqual(discharge_unknown_findings(findings(*args[:2]), *args[:2], proof), [])

    def test_proved_unknown_does_not_hide_typed_identity_or_record_changes(self):
        args = inputs()
        args[0]['native']['codec']['typed'] = [{'id': 'gone'}, {'id': 'reader', 'value': 1}]
        args[1]['native']['codec']['typed'] = [{'id': 'reader', 'value': 2}]
        reduced = discharge_unknown_findings(findings(*args[:2]), *args[:2], unknown_read_witnesses(*args))
        self.assertEqual([row.code for row in reduced], ['native-count', 'native-identity', 'native-record'])

    def test_malformed_reads_fail_closed_instead_of_receiving_credit(self):
        for ranges in [[{'start': 4, 'end': 4}], [{'start': True, 'end': 8}],
                       [{'start': 4, 'end': 6}, {'start': 6, 'end': 8}],
                       [{'start': 4, 'end': 8, 'fake': True}]]:
            args = inputs(); args[3]['fidelity']['record_layout']['reader']['reads']['member'] = ranges
            with self.assertRaises(RetainedEvidenceError): unknown_read_witnesses(*args)

    def test_no_proof_preserves_original_unknown_count_and_identity_findings(self):
        args = inputs(); original = findings(*args[:2])
        self.assertEqual([row.code for row in original], ['rule-b-evidence-required', 'native-identity'])
        self.assertEqual(discharge_unknown_findings(original, *args[:2], []), original)


if __name__ == '__main__':
    unittest.main()
