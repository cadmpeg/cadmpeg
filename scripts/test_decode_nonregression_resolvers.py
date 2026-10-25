#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Controls for §4.4 resolver equality and check 1b."""

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))

import decode_nonregression_resolvers as resolvers  # noqa: E402


def model(**arenas):
    return {'model': arenas, 'native': {}}


def check(base, head, effective, path, resolver):
    return resolvers.resolve(resolvers.Resolvers(base, head, effective), path, resolver)


class SourceObjectControls(unittest.TestCase):
    base = model(curves=[{'id': 'c', 'source_object': {'name': 'EDGE1', 'layer': '7', 'visible': True,
                                                         'color': {'r': 1.0, 'g': 0.0, 'b': 0.0, 'a': 1.0}}}])

    def test_name_resolves_through_the_name_attribute(self):
        head = model(curves=[{'id': 'c'}], attributes=[
            {'id': 'a', 'name': 'name', 'target': {'kind': 'curve', 'id': 'c'}, 'values': [{'kind': 'string', 'value': 'EDGE1'}]}])
        self.assertTrue(check(self.base, head, {}, ('c', 'curves', '#', 'source_object', 'name'), 'effective-source-object'))
        other = model(curves=[{'id': 'c'}])
        self.assertFalse(check(self.base, other, {}, ('c', 'curves', '#', 'source_object', 'name'), 'effective-source-object'))

    def test_layer_visibility_and_colour_resolve_from_the_head_view(self):
        effective = {'records': [{'arena': 'curves', 'id': 'c', 'layers': ['7'], 'visible': True,
                                  'display_color': {'r': 1.0, 'g': 0.0, 'b': 0.0, 'a': 1.0}}]}
        head = model(curves=[{'id': 'c'}])
        for field in ('layer', 'visible', 'color'):
            self.assertTrue(check(self.base, head, effective, ('c', 'curves', '#', 'source_object', field),
                                  'effective-source-object'), field)

    def test_an_absent_visibility_gate_is_not_the_base_value(self):
        effective = {'records': [{'arena': 'curves', 'id': 'c', 'layers': ['7']}]}
        self.assertFalse(check(self.base, model(curves=[{'id': 'c'}]), effective,
                               ('c', 'curves', '#', 'source_object', 'visible'), 'effective-source-object'))


class ValueControls(unittest.TestCase):
    def test_scale_composes_into_the_effective_transform(self):
        rows = [[1.0, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]
        base = model(occurrences=[{'id': 'o', 'transform': rows, 'scale': [2.0, 1.0, 1.0]}])
        good = {'records': [{'arena': 'occurrences', 'id': 'o',
                             'transform': [[2.0, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]}]}
        bad = {'records': [{'arena': 'occurrences', 'id': 'o', 'transform': rows}]}
        path = ('o', 'occurrences', '#', 'scale', '#')
        self.assertTrue(check(base, model(), good, path, 'effective_transform'))
        self.assertFalse(check(base, model(), bad, path, 'effective_transform'))

    def test_expression_renders_with_source_names(self):
        base = model(parameters=[{'id': 'p', 'expression': 'd1 * 2', 'dependencies': ['q']}])
        head = model(parameters=[{'id': 'p', 'expression': {'tokens': [
            {'kind': 'parameter', 'value': 'q'}, {'kind': 'text', 'value': ' * 2'}]}}])
        self.assertTrue(check(base, head, {'expressions': {'p': 'd1 * 2'}}, ('p', 'parameters', '#', 'expression'), 'parameter-resolver'))
        self.assertFalse(check(base, head, {'expressions': {'p': 'd1*2'}}, ('p', 'parameters', '#', 'expression'), 'parameter-resolver'))
        self.assertTrue(check(base, head, {}, ('p', 'parameters', '#', 'dependencies', '#'), 'parameter-resolver'))

    def test_a_non_active_override_must_resolve_to_the_base_text(self):
        base = model(configurations=[{'id': 'k', 'parameter_overrides': {'p': '12 mm'},
                                      'feature_states': {'f': {'evaluation': {'kind': 'suppressed'}}}}])
        effective = {'configurations': {'k': {'parameters': {'p': {'expression': '12 mm', 'value': None}},
                                              'suppressed': {'f': True}}}}
        self.assertTrue(check(base, model(), effective, ('k', 'configurations', '#', 'parameter_overrides', 'p'), 'configuration-resolver'))
        self.assertTrue(check(base, model(), effective, ('k', 'configurations', '#', 'feature_states', 'f', 'evaluation', 'kind'), 'configuration-resolver'))
        emptied = {'configurations': {'k': {'parameters': {'p': {'expression': '10 mm', 'value': None}},
                                            'suppressed': {'f': False}}}}
        self.assertFalse(check(base, model(), emptied, ('k', 'configurations', '#', 'parameter_overrides', 'p'), 'configuration-resolver'))
        self.assertFalse(check(base, model(), emptied, ('k', 'configurations', '#', 'feature_states', 'f', 'evaluation', 'kind'), 'configuration-resolver'))

    def test_a_companion_is_bound_by_one_sibling_dimension_with_the_base_value(self):
        base = model(sketch_constraints=[{'id': 's', 'definition': {'kind': 'distance', 'distance': 4.0, 'parameter': 'p'}}])
        head = model(sketch_constraints=[{'id': 's', 'definition': {'kind': 'distance', 'distance': {'kind': 'parameter', 'value': 'p'}}}])
        effective = {'definitions': {'sketch_constraints': {'s': {'kind': 'distance', 'distance': 4.0}}}}
        path = ('s', 'sketch_constraints', '#', 'definition', 'parameter')
        self.assertTrue(check(base, head, effective, path, 'dimension-resolver'))
        moved = {'definitions': {'sketch_constraints': {'s': {'kind': 'distance', 'distance': 5.0}}}}
        self.assertFalse(check(base, head, moved, path, 'dimension-resolver'))
        unbound = model(sketch_constraints=[{'id': 's', 'definition': {'kind': 'distance', 'distance': {'kind': 'literal', 'value': 4.0}}}])
        self.assertFalse(check(base, unbound, effective, path, 'dimension-resolver'))

    def test_driving_resolves_through_the_bound_parameter(self):
        base = model(sketch_constraints=[{'id': 's', 'driving': False}])
        head = model(sketch_constraints=[{'id': 's', 'definition': {'distance': {'kind': 'parameter', 'value': 'p'}}}],
                     parameters=[{'id': 'p', 'driving': False}])
        self.assertTrue(check(base, head, {}, ('s', 'sketch_constraints', '#', 'driving'), 'dimension-driving'))
        head['model']['parameters'][0]['driving'] = True
        self.assertFalse(check(base, head, {}, ('s', 'sketch_constraints', '#', 'driving'), 'dimension-driving'))

    def test_spreadsheet_cells_resolve_by_address(self):
        base = model(spreadsheets=[{'id': 'x', 'cells': [{'address': 'A1', 'parameter': 'p'}]}],
                     parameters=[{'id': 'p', 'expression': '3', 'value': {'kind': 'real', 'value': 3.0}}])
        head = model(spreadsheets=[{'id': 'x', 'cells': [{'address': 'A1', 'parameter': 'p'}]}],
                     parameters=[{'id': 'p', 'value': {'kind': 'real', 'value': 3.0}}])
        self.assertTrue(check(base, head, {'expressions': {'p': '3'}}, ('x', 'spreadsheets', '#', 'cells', '#', 'address'), 'cell-resolver'))
        self.assertFalse(check(base, head, {'expressions': {'p': '4'}}, ('x', 'spreadsheets', '#', 'cells', '#', 'address'), 'cell-resolver'))


class Check1bControls(unittest.TestCase):
    def test_a_property_survives_in_a_typed_field_an_owned_record_or_a_deletion_list(self):
        base = model(features=[{'id': 'fcstd:f', 'source_properties': {'MapMode': 'Deactivated', 'Gone': '1',
                                                                      'Label': 'Box', 'alignment': 'x'}}])
        head = {'model': {'features': [{'id': 'fcstd:f', 'native_ref': 'fcstd:n'}]},
                'native': {'fcstd': {'objects': [{'id': 'fcstd:n', 'map_mode': 'Deactivated'}],
                                     'properties': [{'id': 'fcstd:p', 'owner': 'fcstd:n', 'name': 'Label', 'value': 'Box'}]}}}
        deletions = {'fcstd': (resolvers.re.compile(r'alignment\Z'),)}
        findings = resolvers.check_1b(base, head, deletions)
        self.assertEqual([finding.path[-1] for finding in findings], ['Gone'])

    def test_section_8_deletions_are_read_per_codec_with_index_patterns(self):
        with tempfile.TemporaryDirectory() as directory:
            names = Path(directory) / 'removed_names.rs'
            names.write_text('/// Removed by #230 §8.\npub const BODY_WRITE_N: &str = "body_write.N";\n'
                             '/// Removed by #246 check 2.\npub const ACTIVE_BREP: &str = "active_brep";\n')
            table = resolvers.section_8_deletions(names)
        self.assertEqual(list(table), ['nx'])
        self.assertTrue(table['nx'][0].match('body_write.12'))
        self.assertFalse(table['nx'][0].match('body_write.x'))

    def test_the_committed_registry_names_section_8_deletions(self):
        self.assertTrue(resolvers.section_8_deletions())


if __name__ == '__main__':
    unittest.main()
