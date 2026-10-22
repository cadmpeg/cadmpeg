#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from decode_nonregression_native_links import NativeLinkError, verify_bound_native_links
from test_decode_nonregression_native_links import inputs


class NativeLinkBindingTests(unittest.TestCase):
    def source(self, directory):
        report, document, _ = inputs()
        path = Path(directory)/'head.json'; path.write_text(json.dumps(document))
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        report.update(command='query', view='native_links')
        report['payload']['ir_sha256'] = digest
        return report, path, document, digest

    def test_actual_bytes_and_exact_inventory_prove_existing_target(self):
        with tempfile.TemporaryDirectory() as directory:
            findings, witnesses = verify_bound_native_links(*self.source(directory))
            self.assertEqual(findings, [])
            self.assertEqual(witnesses[0]['native_ref'], 'target')

    def test_file_changed_during_query_fails_even_when_report_matches_new_file(self):
        with tempfile.TemporaryDirectory() as directory:
            report, path, document, expected = self.source(directory)
            path.write_text(json.dumps(document, indent=2))
            report['payload']['ir_sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
            with self.assertRaises(NativeLinkError): verify_bound_native_links(report, path, document, expected)

    def test_earlier_inventory_cannot_resolve_targets_for_another_source(self):
        with tempfile.TemporaryDirectory() as directory:
            report, path, document, _ = self.source(directory)
            changed = json.loads(path.read_text()); changed['native']['codec']['objects'][0]['id'] = 'other'
            path.write_text(json.dumps(changed)); digest = hashlib.sha256(path.read_bytes()).hexdigest()
            report['payload']['ir_sha256'] = digest
            with self.assertRaises(NativeLinkError): verify_bound_native_links(report, path, document, digest)

    def test_boolean_and_integer_inventory_values_are_not_interchangeable(self):
        with tempfile.TemporaryDirectory() as directory:
            report, path, document, _ = self.source(directory)
            document['model']['attributes'][0]['extra'] = True
            changed = json.loads(json.dumps(document)); changed['model']['attributes'][0]['extra'] = 1
            path.write_text(json.dumps(changed)); digest = hashlib.sha256(path.read_bytes()).hexdigest()
            report['payload']['ir_sha256'] = digest
            with self.assertRaises(NativeLinkError): verify_bound_native_links(report, path, document, digest)

    def test_duplicate_json_keys_and_wrong_query_envelopes_refuse(self):
        with tempfile.TemporaryDirectory() as directory:
            report, path, document, digest = self.source(directory)
            for field, value in [('command', 'dump'), ('view', 'schema')]:
                wrong = dict(report); wrong[field] = value
                with self.assertRaises(NativeLinkError): verify_bound_native_links(wrong, path, document, digest)
            path.write_text('{"model":{},"model":{},"native":{}}')
            digest = hashlib.sha256(path.read_bytes()).hexdigest(); report['payload']['ir_sha256'] = digest
            with self.assertRaises(NativeLinkError): verify_bound_native_links(report, path, {'model':{},'native':{}}, digest)

    def test_file_changed_during_verification_refuses(self):
        with tempfile.TemporaryDirectory() as directory:
            data = self.source(directory); raw = data[1].read_bytes()
            with patch.object(Path, 'read_bytes', side_effect=[raw, raw+b' ']):
                with self.assertRaises(NativeLinkError): verify_bound_native_links(*data)


if __name__ == '__main__':
    unittest.main()
