#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that CI selection retains dependents and conservatively handles inputs."""

from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch

from scripts.ci_scope import select
from scripts.ci_cargo import command, main as cargo_main

ROOT = Path(__file__).resolve().parent.parent


class ScopeTests(unittest.TestCase):
    def test_cargo_command_preserves_arguments(self):
        self.assertEqual(command(['test', '--doc'], ['cadmpeg-core', 'cadmpeg-ir']),
                         ['cargo', 'test', '-q', '-p', 'cadmpeg-core',
                          '-p', 'cadmpeg-ir', '--doc'])
        with self.assertRaises(ValueError):
            command(['test'], [])

    def test_cli_only_has_no_library_doc_tests(self):
        with patch.dict('os.environ', {'CI_PACKAGES': '["cadmpeg"]', 'CI_FULL': 'false'}), \
                patch('sys.argv', ['ci_cargo.py', 'test', '--doc']), \
                patch('scripts.ci_cargo.subprocess.run') as run:
            self.assertEqual(cargo_main(), 0)
            run.assert_not_called()

    def test_documentation_gate_keeps_selected_features(self):
        with patch.dict('os.environ', {'CI_PACKAGES': '["cadmpeg","cadmpeg-ir"]', 'CI_FULL': 'false'}), \
                patch('sys.argv', ['ci_cargo.py', 'test', '--doc']), \
                patch('scripts.ci_cargo.subprocess.run') as run:
            run.return_value.returncode = 0
            self.assertEqual(cargo_main(), 0)
            run.assert_called_once_with(['cargo', 'test', '-q', '-p', 'cadmpeg',
                                         '-p', 'cadmpeg-ir', '--doc'])

    def test_full_gate_uses_cargo_workspace_membership(self):
        self.assertEqual(command(['test', '--bins', '--tests'], [], full=True),
                         ['cargo', 'test', '-q', '--workspace', '--bins', '--tests'])

    def test_package_manifest_changes_keep_feature_unification_coverage(self):
        self.assertTrue(select(ROOT, ['crates/cadmpeg-codec-rhino/Cargo.toml'])['full'])
        scope = select(ROOT, ['crates/cadmpeg-fuzz/Cargo.toml'])
        self.assertFalse(scope['rust'])
        self.assertTrue(scope['features'])

    def test_graph_keeps_renamed_workspace_target_build_and_dev_dependencies(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'Cargo.toml').write_text(
                '[workspace]\nmembers=["crates/*"]\nexclude=["crates/excluded"]\n'
                '[workspace.dependencies]\nalias={package="shared",path="crates/shared"}\n')
            manifests = {
                'shared': '',
                'dev-child': '[dev-dependencies]\nalias={workspace=true}\n',
                'target-child': '[target.\'cfg(windows)\'.dependencies]\nrenamed={package="shared",path="../shared"}\n',
                'build-child': '[build-dependencies]\nshared={path="../shared"}\n',
                'application': '[dependencies]\n"dev-child"={path="../dev-child"}\n"target-child"={path="../target-child"}\n"build-child"={path="../build-child"}\n',
                'excluded': '',
            }
            for name, dependencies in manifests.items():
                directory = root / 'crates' / name
                directory.mkdir(parents=True)
                (directory / 'Cargo.toml').write_text(f'[package]\nname="{name}"\n' + dependencies)
            expected = sorted(set(manifests) - {'excluded'})
            self.assertEqual(select(root, ['crates/shared/src/lib.rs'])['packages'], expected)
            self.assertEqual(select(root, [], full=True)['packages'], expected)

    def test_bounded_gate_tool_change_selects_iges(self):
        scope = select(ROOT, ['scripts/verify-iges-bounded.py'])
        self.assertIn('cadmpeg-codec-iges', scope['packages'])
        self.assertTrue(scope['iges'])

    def test_inventor_tool_keeps_cli_evidence_coverage(self):
        for path in ('scripts/inventor-evidence.py', 'scripts/test_inventor_evidence.py'):
            scope = select(ROOT, [path])
            self.assertIn('cadmpeg-codec-inventor', scope['packages'])
            self.assertIn('cadmpeg', scope['packages'])
            self.assertTrue(scope['rust'])
            self.assertFalse(scope['iges'])

    def test_python_only_skips_rust(self):
        scope = select(ROOT, ['scripts/check-deny-census.py'])
        self.assertFalse(scope['rust'])
        self.assertFalse(scope['features'])

    def test_codec_selects_facade_and_cli_dependents(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-iges/src/reader.rs'])
        self.assertIn('cadmpeg-codec-iges', scope['packages'])
        self.assertIn('cadmpeg-registry', scope['packages'])
        self.assertIn('cadmpeg', scope['packages'])
        self.assertNotIn('cadmpeg-codec-step', scope['packages'])
        self.assertTrue(scope['iges'])
        self.assertTrue(scope['features'])

    def test_other_codec_does_not_repeat_bounded_iges_gate(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-rhino/src/reader.rs'])
        self.assertIn('cadmpeg', scope['packages'])
        self.assertFalse(scope['iges'])
        for owner in ('cadmpeg', 'cadmpeg-registry'):
            self.assertTrue(select(ROOT, [f'crates/{owner}/src/main.rs'])['iges'])

    def test_core_selects_every_codec(self):
        scope = select(ROOT, ['crates/cadmpeg-core/src/decode/context.rs'])
        full = select(ROOT, [], full=True)
        self.assertEqual(scope['packages'], full['packages'])
        self.assertTrue(scope['schema'])

    def test_fuzz_only_keeps_independent_workspace_gate(self):
        scope = select(ROOT, ['crates/cadmpeg-fuzz/fuzz_targets/iges.rs'])
        self.assertFalse(scope['rust'])
        self.assertTrue(scope['features'])
        self.assertFalse(scope['schema'])

    def test_move_includes_both_old_and_new_owners(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-step/src/removed.rs',
                              'crates/cadmpeg-codec-iges/src/added.rs'])
        self.assertIn('cadmpeg-codec-step', scope['packages'])
        self.assertIn('cadmpeg-codec-iges', scope['packages'])

    def test_fixture_is_owned_by_codec(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-step/tests/golden/fixtures/input.step'])
        self.assertIn('cadmpeg-codec-step', scope['packages'])
        self.assertIn('cadmpeg', scope['packages'])

    def test_shared_and_unknown_inputs_use_full_gate(self):
        for path in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                     '.cargo/config.toml', '.github/workflows/ci.yml',
                     'docs/layouts/iges.toml', 'corpus/iges-envelope-a.toml',
                     'scripts/generator.sh', 'scripts/check-unclassified.py',
                     'scripts/set-workspace-version.py',
                     'scripts/ci_scope.py', 'scripts/test_ci_scope.py', 'unknown.txt']:
            with self.subTest(path=path):
                self.assertTrue(select(ROOT, [path])['full'])

    def test_main_uses_full_gate_even_for_python(self):
        scope = select(ROOT, ['scripts/check-deny-census.py'], full=True)
        self.assertTrue(scope['full'])
        self.assertTrue(scope['rust'])
        self.assertTrue(scope['schema'])


if __name__ == '__main__':
    unittest.main()
