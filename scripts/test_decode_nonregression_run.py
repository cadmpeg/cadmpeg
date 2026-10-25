#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Controls for the §4.4 base/head collection and comparison driver."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))

import decode_nonregression_run as run  # noqa: E402
from decode_nonregression_schema import GeneratedArena  # noqa: E402

SCRIPT = Path(__file__).with_name('check-decode-nonregression.py')
SPEC = importlib.util.spec_from_file_location('decode_nonregression_driver', SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)

FEATURES = {
    'status': 'ok',
    'payload': {
        'arena': 'model.features',
        'element': 'Feature',
        'schema': {
            'type': 'object',
            'required': ['id'],
            'properties': {
                'id': {'type': 'string'},
                'value': {'type': ['number', 'null']},
                'definition': {'oneOf': [
                    {'type': 'object', 'required': ['kind'], 'properties': {'kind': {'type': 'string', 'const': 'extrude'}}},
                    {'type': 'object', 'required': ['kind'], 'properties': {'kind': {'type': 'string', 'const': 'unknown'}}},
                ]},
            },
        },
        'defs': {},
    },
}


class FixedSchemaBuild:
    role = 'test'

    def arena(self, name):
        if name != 'features':
            raise AssertionError(name)
        return GeneratedArena(FEATURES)


def document(features, native=None):
    return {'ir_version': '8', 'model': {'features': features}, 'native': native or {}}


def sidecar(record_layout=None, losses=()):
    return {'ir_version': '8', 'ir_sha256': '0' * 64,
            'report': {'losses': [{'code': {'code': code, 'namespace': 'decode'}} for code in losses]},
            'fidelity': {'retained_records': {}, 'record_layout': record_layout or {}}}


def layout(pointers=None, payloads=None):
    return {'stream': 's', 'offset': 0, 'byte_len': 8, 'reads': {},
            'pointers': pointers or {}, 'payloads': payloads or {}}


def compare(base, head, base_side=None, head_side=None, moves=()):
    build = FixedSchemaBuild()
    return run.compare_pair(checker, base, head, base_side or sidecar(), head_side or sidecar(), build, build, moves)


class DriverControls(unittest.TestCase):
    def test_appendix_b_rows_become_exact_native_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            table = Path(directory) / 'rows.tsv'
            table.write_text(
                'codec\tjson_path\trust_location\ttype\tclass\tbasis\n'
                'x\tnative/x/rows/#/offset; native/x/other/#/nested/offset\tf.rs [A.offset]\tu64\tmove-layout\tb\n'
                'x\tnative/x/rows/#/payload\tf.rs [A.payload]\tNativeBytes\tmove-payload\tb\n'
                'x\tnative/x/rows/#/kept\tf.rs [A.kept]\tNativeBytes\tkeep\tb\n'
                'x\t(unpopulated) rows\tf.rs [A.gone]\tu64\tmove-layout\tb\n')
            moves = run.appendix_b_moves(table)
        self.assertEqual(moves, (
            run.Move('x', 'other', ('nested', 'offset'), 'pointers'),
            run.Move('x', 'rows', ('offset',), 'pointers'),
            run.Move('x', 'rows', ('payload',), 'payloads'),
        ))

    def test_committed_appendix_b_names_no_unknown_store(self):
        moves = run.appendix_b_moves()
        self.assertTrue(moves)
        self.assertEqual({move.store for move in moves}, {'pointers', 'payloads'})

    def test_moved_payload_must_equal_the_stored_layout_payload(self):
        native = {'x': {'rows': [{'id': 'r', 'payload': 'abcd', 'value': 1}]}}
        head = {'x': {'rows': [{'id': 'r', 'value': 1}]}}
        moves = (run.Move('x', 'rows', ('payload',), 'payloads'),)
        stored = sidecar({'r': layout(payloads={'payload': 'abcd'})})
        self.assertEqual(compare(document([], native), document([], head), head_side=stored, moves=moves), [])
        wrong = sidecar({'r': layout(payloads={'payload': 'abce'})})
        findings = compare(document([], native), document([], head), head_side=wrong, moves=moves)
        self.assertIn('layout-value-preservation', [finding.code for finding in findings])

    def test_a_layout_move_is_not_a_payload_move(self):
        native = {'x': {'rows': [{'id': 'r', 'offset': 4}]}}
        head = {'x': {'rows': [{'id': 'r'}]}}
        moves = (run.Move('x', 'rows', ('offset',), 'pointers'),)
        payload_only = sidecar({'r': layout(payloads={'offset': '04'})})
        findings = compare(document([], native), document([], head), head_side=payload_only, moves=moves)
        self.assertIn('layout-value-preservation', [finding.code for finding in findings])

    def test_head_decode_error_fails_only_where_base_decodes(self):
        failed = {'decode_error': 'cadmpeg dump exited 1'}
        self.assertEqual([f.code for f in compare(document([]), failed)], ['head-decode-error'])
        self.assertEqual(compare(failed, failed), [])

    def test_dropped_neutral_record_and_leaf_fail(self):
        base = document([{'id': 'a', 'value': 1.0}, {'id': 'b', 'value': 2.0}])
        head = document([{'id': 'a', 'value': None}])
        codes = sorted(f.code for f in compare(base, head))
        self.assertEqual(codes, ['neutral-count', 'neutral-leaf', 'neutral-record'])

    def test_more_uncertain_variants_fail(self):
        base = document([{'id': 'a', 'definition': {'kind': 'extrude'}}])
        head = document([{'id': 'a', 'definition': {'kind': 'unknown'}}])
        codes = {f.code for f in compare(base, head)}
        self.assertIn('uncertain-variant-increase', codes)

    def test_a_record_the_schema_rejects_keeps_its_tag_paths(self):
        arena = GeneratedArena(FEATURES)
        record = {'id': 'a', 'definition': {'kind': 'extrude'}, 'unlisted': 1}
        with self.assertRaises(run.GeneratedSchemaError):
            arena.paths([record])
        self.assertIn(('features', '#', 'definition', 'kind'), run.classify(arena, record).variant_tags)
        base = document([record])
        head = document([{'id': 'a', 'definition': {'kind': 'unknown'}, 'unlisted': 1}])
        self.assertIn('neutral-tag', {f.code for f in compare(base, head)})

    def test_a_removed_failure_record_needs_its_given_up_part(self):
        failed = {'id': 'iges:quarantine:parameter#3', 'source_offset': 40, 'bytes': '3132'}
        base = document([], {'iges': {'quarantined_parameter_records': [failed]}})
        head = document([], {'iges': {'unknowns': [{'id': 'iges:source:part#a'}]}})
        stub = {'stream': 'iges/parameter', 'offset': 40, 'bytes': {'retention': 'inline', 'data': 'MTI='}}
        loss = {'code': {'code': 'record.unreadable'}, 'message': 'p stream=iges/parameter offset=40 length=2'}
        given_up = sidecar()
        given_up['fidelity']['retained_records'] = {'iges:source:part#a': stub}
        given_up['report']['losses'] = [loss]
        findings = run.failure_findings(checker, checker.compare_native(base['native'], head['native'], checker.Evidence()),
                                        base, head, given_up)
        self.assertEqual(findings, [])
        silent = sidecar()
        silent['fidelity']['retained_records'] = {'iges:source:part#a': stub}
        findings = run.failure_findings(checker, [], base, head, silent)
        self.assertEqual([finding.code for finding in findings], ['failure-record-preservation'])

    def test_a_failure_row_leaves_its_arena_without_a_count_finding(self):
        rows = [{'id': 'rhino:document:setting#units'}, {'id': 'rhino:document:setting#error-1', 'parse_error': 'x'}]
        base = document([], {'rhino': {'setting_records': rows}})
        head = document([], {'rhino': {'setting_records': rows[:1]}})
        findings = run.failure_findings(checker, checker.compare_native(base['native'], head['native'], checker.Evidence()),
                                        base, head, sidecar())
        self.assertEqual([finding.code for finding in findings], ['failure-record-preservation'])

    def test_a_gone_arena_needs_no_preservation_proof(self):
        base = document([], {'fcstd': {'logical_ledger': [{'id': 'fcstd:native:logical-span#0', 'start': 0}]}})
        head = document([], {'fcstd': {}})
        findings = run.failure_findings(checker, checker.compare_native(base['native'], head['native'], checker.Evidence()),
                                        base, head, sidecar())
        self.assertEqual(findings, [])

    def test_an_exempt_path_needs_its_resolver_value(self):
        base = {'model': {'parameters': [{'id': 'p', 'expression': 'd1 * 2'}]}, 'native': {}}
        head = {'model': {'parameters': [{'id': 'p', 'expression': {'tokens': [{'kind': 'text', 'value': 'd1 * 2'}]}}]}, 'native': {}}
        finding = checker.Finding('resolver-evidence-required', ('p', 'parameters', '#', 'expression'), '')
        self.assertEqual(run.resolve_findings(checker, [finding], base, head, {'expressions': {'p': 'd1 * 2'}}), [])
        self.assertEqual([f.code for f in run.resolve_findings(checker, [finding], base, head, None)], ['resolver-mismatch'])
        unresolved = checker.Finding('resolver-evidence-required', ('f', 'features', '#', 'dependencies', '#'), '')
        self.assertEqual(run.resolve_findings(checker, [unresolved], base, head, None), [])

    def test_common_features_keep_their_base_ordinal_order(self):
        base = document([{'id': 'a', 'ordinal': 0}, {'id': 'b', 'ordinal': 1}, {'id': 'c', 'ordinal': 2}])
        kept = document([{'id': 'a', 'ordinal': 0}, {'id': 'x', 'ordinal': 1}, {'id': 'b', 'ordinal': 2}, {'id': 'c', 'ordinal': 3}])
        self.assertEqual(run.ordinal_order_findings(checker, base, kept), [])
        swapped = document([{'id': 'a', 'ordinal': 0}, {'id': 'c', 'ordinal': 1}, {'id': 'b', 'ordinal': 2}])
        self.assertEqual([f.path[-1] for f in run.ordinal_order_findings(checker, base, swapped)], ['c'])

    def test_a_new_reference_to_a_b1_span_fails(self):
        stub = {'id': 'x:source:part#1', 'links': ['x:native:r#1']}
        base = document([{'id': 'a'}], {'x': {'unknowns': [stub]}})
        head = document([{'id': 'a', 'source': 'x:source:part#1'}], {'x': {'unknowns': [stub]}})
        codes = [f.code for f in run.span_reference_findings(checker, base, head, sidecar())]
        self.assertEqual(codes, ['b1-span-reference'])
        self.assertEqual(run.span_reference_findings(checker, head, head, sidecar()), [])

    def test_an_added_string_only_variant_fails(self):
        def query(branches):
            return {'status': 'ok', 'payload': {'arena': 'model.features', 'element': 'Feature',
                    'schema': {'type': 'object', 'properties': {'definition': {'$ref': '#/$defs/Definition'}}},
                    'defs': {'Definition': {'oneOf': branches}}}}
        typed = {'type': 'object', 'required': ['kind'], 'properties': {
            'kind': {'type': 'string', 'const': 'extrude'}, 'depth': {'type': 'number'}}}
        loose = {'type': 'object', 'required': ['kind'], 'properties': {
            'kind': {'type': 'string', 'const': 'loose'}, 'text': {'type': 'string'},
            'values': {'type': 'object', 'additionalProperties': {'type': 'string'}}}}

        class SchemaBuild:
            def __init__(self, branches):
                self.branches = branches

            def run(self, arguments):
                return '{"payload": {"model.features": "Feature"}}'

            def arena(self, name):
                return GeneratedArena(query(self.branches))

        reference = {'type': 'object', 'required': ['kind'], 'properties': {
            'kind': {'type': 'string', 'const': 'reference'}, 'id': {'type': 'string', 'pattern': '^x:y:z#.+$'}}}
        findings = run.variant_guard_findings(checker, SchemaBuild([typed]), SchemaBuild([typed, loose, reference]))
        self.assertEqual([f.path for f in findings], [('schema', 'Definition', 'loose')])
        self.assertEqual(run.variant_guard_findings(checker, SchemaBuild([typed, loose]), SchemaBuild([typed, loose])), [])

    def test_missing_sidecar_is_a_finding(self):
        build = FixedSchemaBuild()
        findings = run.compare_pair(checker, document([]), document([]), None, None, build, build, ())
        self.assertEqual([f.code for f in findings], ['decode-sidecar-missing'])

    def test_new_golden_outside_damaged_charges_no_unreadable_record(self):
        charged = sidecar(losses=['record.unreadable'])
        self.assertEqual([f.code for f in run.new_golden_findings(checker, 'crates/c/tests/golden/fixtures/a.igs', charged)],
                         ['new-golden-unreadable'])
        self.assertEqual(run.new_golden_findings(checker, 'crates/c/tests/golden/fixtures/damaged/a.igs', charged), [])
        self.assertEqual(run.new_golden_findings(checker, 'crates/c/tests/golden/fixtures/a.igs', sidecar()), [])

    def test_decode_goldens_are_selected_by_location(self):
        self.assertTrue(run.is_decode_golden('crates/cadmpeg-codec-sat/tests/golden/decode/text_sphere.json'))
        self.assertTrue(run.is_decode_golden('crates/cadmpeg-codec-nx/tests/golden/assembly_prt.json'))
        self.assertFalse(run.is_decode_golden('crates/cadmpeg-codec-sat/tests/golden/inspect/text_sphere.json'))
        self.assertFalse(run.is_decode_golden('crates/cadmpeg-codec-rhino/tests/golden/encode/a.json'))
        self.assertEqual(run.golden_key('crates/c/tests/golden/decode/damaged/a.json'),
                         run.golden_key('crates/c/tests/golden/decode/a.json'))

    def test_golden_parts_read_both_golden_shapes(self):
        flat = {'ir': {'model': {}}, 'report': {'losses': []}, 'source_fidelity': {'x': 1}}
        nested = {'decode': flat, 'inspect': {}}
        for golden in (flat, nested):
            document_part, side = run.golden_parts(golden)
            self.assertEqual(document_part, {'model': {}})
            self.assertEqual(side['fidelity'], {'x': 1})

    def test_elided_native_arenas_compare_by_count_and_pinned_values(self):
        def elided(count, values):
            return {'__elided': 'marker', '__arena_counts': {'x': {'rows': count}},
                    '__arena_sha256': {}, '__arena_values': values}
        base = document([], elided(2, {}))
        self.assertEqual(compare(base, document([], elided(2, {}))), [])
        self.assertEqual([f.code for f in compare(base, document([], elided(1, {})))], ['native-count'])
        pinned = elided(1, {'x': {'kept': [{'id': 'k', 'value': 1}]}})
        changed = elided(1, {'x': {'kept': [{'id': 'k', 'value': 2}]}})
        self.assertEqual([f.code for f in compare(document([], pinned), document([], changed))], ['native-record'])


if __name__ == '__main__':
    unittest.main()
