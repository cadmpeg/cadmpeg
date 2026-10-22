#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Retained-image integrity controls, not caller-supplied move exemptions."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from decode_nonregression_retained import compare_retained, images, load_bound_sidecar, sidecar_path, RetainedEvidenceError


def image(data=b'ABCD', stream='member', offset=4):
    return {'stream': stream, 'offset': offset,
            'bytes': {'retention': 'inline', 'data': base64.b64encode(data).decode()}}


def sidecar(record=None):
    return {'fidelity': {'retained_records': {'record': image() if record is None else record}}}


class RetainedEvidenceTests(unittest.TestCase):
    def test_identical_images_and_added_head_images_preserve_data(self):
        old, new = sidecar(), sidecar()
        new['fidelity']['retained_records']['added'] = image(b'extra')
        self.assertEqual(compare_retained(old, new), ([], 1))

    def test_equal_length_changed_bytes_fail(self):
        findings, count = compare_retained(sidecar(), sidecar(image(b'ABCE')))
        self.assertEqual(count, 1)
        self.assertEqual([row.code for row in findings], ['retained-source-bytes'])

    def test_available_bytes_never_become_digest_only_without_consumption_proof(self):
        record = image(); record['bytes'] = {'retention': 'digest', 'byte_len': 4,
                                            'sha256': hashlib.sha256(b'ABCD').hexdigest()}
        self.assertEqual(compare_retained(sidecar(), sidecar(record))[0][0].code, 'retained-source-unavailable')
        self.assertEqual(compare_retained(sidecar(record), sidecar()), ([], 1))
        self.assertEqual(compare_retained(sidecar(record), sidecar(record)), ([], 1))

    def test_source_coordinates_are_not_inferred_from_matching_content(self):
        for changed in [image(stream='another'), image(offset=5), image(stream='')]:
            findings, _ = compare_retained(sidecar(), sidecar(changed))
            self.assertEqual(findings[0].code, 'retained-source-location')

    def test_retention_layout_or_fake_read_does_not_excuse_a_removed_image(self):
        head = {'fidelity': {'retained_records': {}, 'record_layout': {'owner': {
            'stream': 'member', 'offset': 0, 'byte_len': 100,
            'reads': {'member': [{'start': 4, 'end': 8}]}, 'payloads': {'copy': '41424344'}, 'pointers': {}}}}}
        findings, _ = compare_retained(sidecar(), head)
        self.assertEqual(findings[0].code, 'retained-consumption-proof-required')

    def test_malformed_bytes_unknown_fields_and_extents_fail_closed(self):
        records = []
        unknown = image(); unknown['ignored'] = True; records.append(unknown)
        invalid = image(); invalid['bytes']['data'] = '**'; records.append(invalid)
        variant = image(); variant['bytes']['length'] = 4; records.append(variant)
        negative = image(offset=-1); records.append(negative)
        boolean = image(offset=True); records.append(boolean)
        overflow = image(offset=2**64-1); records.append(overflow)
        for record in records:
            with self.assertRaises(RetainedEvidenceError): images(sidecar(record))

    def test_digest_evidence_requires_exact_length_and_valid_digest(self):
        record = image(); record['bytes'] = {'retention': 'digest', 'byte_len': 4,
                                            'sha256': hashlib.sha256(b'ABCD').hexdigest()}
        changed = copy.deepcopy(record); changed['bytes']['byte_len'] = 5
        self.assertEqual(compare_retained(sidecar(record), sidecar(changed))[0][0].code, 'retained-source-bytes')
        for field, value in [('sha256', 'not-a-digest'), ('byte_len', True)]:
            invalid = copy.deepcopy(record); invalid['bytes'][field] = value
            with self.assertRaises(RetainedEvidenceError): images(sidecar(invalid))

    def test_exact_cadir_hash_version_and_closed_sidecar_root_are_required(self):
        with tempfile.TemporaryDirectory() as directory:
            document = Path(directory)/'input.json'
            document.write_bytes(b'{"ir_version":"current"}\n')
            wire = {'ir_version': 'current', 'ir_sha256': hashlib.sha256(document.read_bytes()).hexdigest(),
                    'report': {}, 'fidelity': {'retained_records': {}}}
            path = sidecar_path(document)
            path.write_text(json.dumps(wire))
            self.assertEqual(load_bound_sidecar(document, {'ir_version': 'current'}), wire)
            for key, value in [('ir_sha256', '0'*64), ('ir_version', 'other'), ('unknown', True)]:
                damaged = dict(wire); damaged[key] = value; path.write_text(json.dumps(damaged))
                with self.assertRaises(RetainedEvidenceError): load_bound_sidecar(document, {'ir_version': 'current'})
            path.write_text('{"ir_version":"current","ir_version":"current"}')
            with self.assertRaisesRegex(RetainedEvidenceError, 'duplicate'):
                load_bound_sidecar(document, {'ir_version': 'current'})

    def test_sidecar_suffix_follows_actual_public_path_contract(self):
        self.assertEqual(sidecar_path(Path('part.cadir.json')), Path('part.cadir.fidelity.json'))
        self.assertEqual(sidecar_path(Path('part.cadir')), Path('part.cadir.fidelity.json'))


if __name__ == '__main__':
    unittest.main()
