#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Collector guard and handoff controls; fake producers are not corpus proof."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from decode_nonregression_collect import CollectionError, Producer, compare_collected, digest
from test_decode_nonregression_d10 import inputs

spec = importlib.util.spec_from_file_location('collector_comparator', Path(__file__).with_name('check-decode-nonregression.py'))
comparator = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = comparator
spec.loader.exec_module(comparator)


class CollectorTests(unittest.TestCase):
    def test_binary_replacement_is_rejected_before_a_command(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'binary'
            binary.write_bytes(b'original')
            producer = Producer(Path(directory), {'binary': str(binary), 'binary_sha256': digest(binary)})
            binary.write_bytes(b'replacement')
            with self.assertRaisesRegex(CollectionError, 'executable changed'):
                producer.run_json(['query'])
            self.assertEqual(producer.commands, [])

    def test_metadata_from_another_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'build').write_text(json.dumps({'tree': 'tree', 'commit': 'commit'}))
            (root / 'schema').write_text(json.dumps({'binary_sha256': 'other'}))
            (root / 'proof').write_text(json.dumps({'protocol': 'cadmpeg-local-producer-v1', 'role': 'head',
                'CI_evidence': False, 'commit': 'commit', 'tree': 'tree', 'binary_sha256': 'wanted',
                'build_evidence': str(root / 'build'), 'schema_proofs': str(root / 'schema')}))
            with patch('decode_nonregression_collect.git', return_value='tree'):
                with self.assertRaisesRegex(CollectionError, 'another executable'):
                    Producer.load(root, root / 'proof', 'head')

    def test_actual_typed_pair_discharges_only_its_record_companion(self):
        report, head, base_query, head_query, parameters = inputs()
        base_record = dict(head['model']['sketch_constraints'][0])
        base_record['definition'] = dict(base_record['definition'], distance=3.0, parameter='p')
        base = {'model': {'sketch_constraints': [base_record]}}
        class TypedProducer:
            def __init__(self, query): self.query = query
            def schema(self, arena, helper): return parameters if arena == 'parameters' else self.query
        class Helper:
            def run(self, args, document):
                self.args, self.document = args, document
                return report
        helper = Helper()
        findings = compare_collected(comparator, base, head, TypedProducer(base_query), TypedProducer(head_query), helper)
        self.assertEqual(helper.args, ['--d10', 'r'])
        self.assertIs(helper.document, base)
        self.assertEqual([finding.code for finding in findings], ['coverage-incomplete'])
        changed = json.loads(json.dumps(head))
        changed['model']['parameters'][0]['value']['value'] = 4.0
        with self.assertRaises(ValueError):
            compare_collected(comparator, base, changed, TypedProducer(base_query), TypedProducer(head_query), helper)

    def test_schema_sides_are_not_collapsed(self):
        base = {'items': [{'id': 'r', 'value': 3.0}]}
        head = {'items': [{'id': 'r', 'value': {'kind': 'literal', 'value': 3.0}}]}
        paths = comparator.SchemaPaths(frozenset(), frozenset())
        dimensional = comparator.SchemaPaths(frozenset(), frozenset({('items', '#', 'value')}))
        self.assertEqual(comparator.compare_neutral(base, head, paths, dimensional), [])
        self.assertNotEqual(comparator.compare_neutral(head, base, dimensional, paths),
                            comparator.compare_neutral(head, base, paths, paths))

    def test_base_decode_error_is_a_visible_failure_not_a_skipped_comparison(self):
        result = compare_collected(comparator, {'decode_error': 'bad'}, {}, None, None, None)
        self.assertEqual([finding.code for finding in result], ['baseline-decode-error'])


if __name__ == '__main__':
    unittest.main()
