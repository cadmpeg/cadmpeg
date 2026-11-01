#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that CI selection retains dependents and conservatively handles inputs."""

from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch

from scripts.ci_scope import lock_dependents, select
from scripts.ci_cargo import command, main as cargo_main

ROOT = Path(__file__).resolve().parent.parent


class ScopeTests(unittest.TestCase):
    def test_cargo_command_preserves_arguments(self):
        self.assertEqual(command(['test', '--doc'], ['cadmpeg-core', 'cadmpeg-ir']),
                         ['cargo', 'test', '-q', '-p', 'cadmpeg-core',
                          '-p', 'cadmpeg-ir', '--doc'])
        with self.assertRaises(ValueError):
            command(['test'], [])

    def test_nextest_command_keeps_its_subcommand_before_the_scope(self):
        self.assertEqual(command(['nextest', 'run', '--no-fail-fast'], ['cadmpeg-ir']),
                         ['cargo', 'nextest', 'run', '--cargo-quiet', '--status-level', 'fail',
                          '-p', 'cadmpeg-ir', '--no-fail-fast'])

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

    def test_codec_features_use_registry_feature_names(self):
        result = command(['check'], ['cadmpeg', 'cadmpeg-registry'], codecs=['freecad', 'inventor'])
        self.assertIn('--no-default-features', result)
        self.assertIn('cadmpeg/fcstd,cadmpeg/inventor', result)

    def test_documentation_source_scan_requires_only_prose(self):
        self.assertTrue(select(ROOT, ['docs/formats/f3d.md'])['prose_only'])
        self.assertFalse(select(ROOT, ['docs/formats/f3d.md', 'crates/cadmpeg-core/src/lib.rs'])['prose_only'])
        self.assertFalse(select(ROOT, ['docs/decode-resource-caps.md'])['prose_only'])

    def test_codec_only_fuzz_scope_widens_for_shared_inputs(self):
        self.assertEqual(select(ROOT, ['crates/cadmpeg-codec-inventor/src/lib.rs'])['codec_scope'], ['inventor'])
        self.assertEqual(select(ROOT, ['crates/cadmpeg-core/src/lib.rs'])['codec_scope'], [])
        self.assertEqual(select(ROOT, ['crates/cadmpeg-codec-step/src/lib.rs'])['codec_scope'],
                         ['f3d', 'freecad', 'step'])

    def test_package_manifest_changes_keep_feature_unification_coverage(self):
        self.assertTrue(select(ROOT, ['crates/cadmpeg-codec-rhino/Cargo.toml'])['full'])
        scope = select(ROOT, ['crates/cadmpeg-fuzz/Cargo.toml'])
        self.assertFalse(scope['rust'])
        self.assertTrue(scope['fuzz'])

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
        self.assertFalse(scope['fuzz'])

    def test_preflight_tools_skip_rust(self):
        scope = select(ROOT, ['scripts/preflight.py', 'scripts/preflight_branch.py',
                              'scripts/test_preflight_branch.py'])
        self.assertFalse(scope['rust'])
        self.assertFalse(scope['full'])

    def test_only_named_preflight_tools_skip_rust(self):
        for path in ['scripts/preflight-data.json', 'scripts/preflight/helper.py',
                     'scripts/sub/preflight_x.py', 'scripts/preflightx.py']:
            with self.subTest(path=path):
                self.assertTrue(select(ROOT, [path])['full'])

    def test_codec_selects_facade_and_cli_dependents(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-iges/src/reader.rs'])
        self.assertIn('cadmpeg-codec-iges', scope['packages'])
        self.assertIn('cadmpeg-registry', scope['packages'])
        self.assertIn('cadmpeg', scope['packages'])
        self.assertNotIn('cadmpeg-codec-step', scope['packages'])
        self.assertTrue(scope['iges'])
        self.assertTrue(scope['fuzz'])

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
        self.assertTrue(scope['fuzz'])
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
                     '.github/actions/pinned-rust/action.yml',
                     'docs/formats-index.toml', 'corpus/iges-envelope-a.toml',
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

    def test_layout_tables_and_specifications_select_the_layout_table_test(self):
        for path in ('docs/layouts/iges.toml', 'docs/formats/step.md'):
            with self.subTest(path=path):
                scope = select(ROOT, [path])
                self.assertFalse(scope['full'])
                self.assertEqual(scope['packages'], [])
                self.assertTrue(scope['layout'])

    def test_embedded_document_selects_its_reader(self):
        scope = select(ROOT, ['docs/dialect-id-conformance.toml'])
        self.assertFalse(scope['full'])
        self.assertIn('cadmpeg-core', scope['packages'])

    def test_measurement_contract_selects_measure_crate(self):
        scope = select(ROOT, ['docs/measurement.md'])
        self.assertFalse(scope['full'])
        self.assertIn('cadmpeg-measure', scope['packages'])
        self.assertTrue(scope['rust'])

    def test_prose_and_static_tables_skip_rust(self):
        for path in ('docs/architecture.md', 'README.md', 'docs/codec-facade.toml',
                     '.github/dependabot.yml', 'crates/cadmpeg-codec-inventor/README.md'):
            with self.subTest(path=path):
                scope = select(ROOT, [path])
                self.assertFalse(scope['rust'])
                self.assertFalse(scope['fuzz'])
                self.assertTrue(scope['static'])

    def test_workflows_outside_the_gate_skip_rust(self):
        for name in ('release.yml', 'cut-release.yml', 'audit.yml', 'codeql.yml', 'dco.yml',
                     'new-workflow.yml'):
            with self.subTest(name=name):
                self.assertFalse(select(ROOT, [f'.github/workflows/{name}'])['rust'])

    def test_workflow_called_by_ci_uses_full_gate(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'Cargo.toml').write_text('[workspace]\nmembers=[]\n')
            workflow = root / '.github/workflows/ci.yml'
            workflow.parent.mkdir(parents=True)
            workflow.write_text('jobs:\n  build:\n    uses: ./.github/workflows/build.yml\n')
            self.assertTrue(select(root, ['.github/workflows/build.yml'])['full'])

    def test_lock_dependents_follow_the_resolved_graph(self):
        old = (
            'version = 4\n'
            '[[package]]\nname = "app"\nversion = "1.0.0"\ndependencies = ["lib"]\n'
            '[[package]]\nname = "lib"\nversion = "1.0.0"\ndependencies = ["leaf"]\n'
            '[[package]]\nname = "other"\nversion = "1.0.0"\ndependencies = ["unrelated"]\n'
            '[[package]]\nname = "leaf"\nversion = "0.1.0"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            '[[package]]\nname = "unrelated"\nversion = "0.1.0"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n')
        names = {'app', 'lib', 'other'}
        bumped = old.replace('name = "leaf"\nversion = "0.1.0"', 'name = "leaf"\nversion = "0.1.1"')
        self.assertEqual(lock_dependents(old, bumped, names), {'app', 'lib'})
        member_version = old.replace('name = "lib"\nversion = "1.0.0"', 'name = "lib"\nversion = "1.1.0"')
        self.assertEqual(lock_dependents(old, member_version, names), set())
        self.assertEqual(lock_dependents(old, old, names), set())

    def test_lock_without_a_base_uses_full_gate(self):
        self.assertTrue(select(ROOT, ['Cargo.lock'])['full'])

    def test_unchanged_lock_entries_select_no_package(self):
        lock = (ROOT / 'Cargo.lock').read_text()
        scope = select(ROOT, ['Cargo.lock'], base_lock=lock)
        self.assertFalse(scope['full'])
        self.assertEqual(scope['packages'], [])
        self.assertTrue(scope['audit'])

    def test_audit_follows_dependency_inputs(self):
        self.assertFalse(select(ROOT, ['crates/cadmpeg-codec-step/src/lib.rs'])['audit'])
        self.assertTrue(select(ROOT, ['crates/cadmpeg-codec-step/Cargo.toml'])['audit'])

    def test_cache_writer_widens_cache_key_changes_to_full_gate(self):
        lock = (ROOT / 'Cargo.lock').read_text()
        self.assertTrue(select(ROOT, ['Cargo.lock'], base_lock=lock, cache_writer=True)['full'])
        self.assertTrue(select(ROOT, ['rust-toolchain.toml'], cache_writer=True)['full'])
        scope = select(ROOT, ['crates/cadmpeg-codec-rhino/src/reader.rs'], cache_writer=True)
        self.assertFalse(scope['full'])
        self.assertIn('cadmpeg-codec-rhino', scope['packages'])

    def test_tested_commit_only_refreshes_cache(self):
        scope = select(ROOT, ['crates/cadmpeg-codec-rhino/src/reader.rs'], tested=True)
        self.assertFalse(scope['rust'])
        self.assertFalse(scope['fuzz'])
        self.assertFalse(scope['static'])
        self.assertFalse(scope['audit'])
        self.assertTrue(select(ROOT, ['Cargo.toml'], tested=True)['full'])

    def test_single_codec_gates_follow_their_build_inputs(self):
        other = select(ROOT, ['crates/cadmpeg-codec-f3d/src/lib.rs'])
        self.assertFalse(other['sldprt_cli'])
        self.assertFalse(other['step_cli'])
        step = select(ROOT, ['crates/cadmpeg-codec-step/src/lib.rs'])
        self.assertFalse(step['sldprt_cli'])
        self.assertTrue(step['step_cli'])
        for path in ('crates/cadmpeg-registry/src/lib.rs', 'crates/cadmpeg-core/src/lib.rs'):
            with self.subTest(path=path):
                scope = select(ROOT, [path])
                self.assertTrue(scope['sldprt_cli'])
                self.assertTrue(scope['step_cli'])

    def test_specification_edit_runs_only_the_layout_table_test(self):
        scope = select(ROOT, ['docs/formats/f3d.md'])
        self.assertEqual(scope['packages'], [])
        self.assertTrue(scope['layout'])
        for output in ('fuzz', 'sldprt_cli', 'step_cli', 'iges', 'schema'):
            with self.subTest(output=output):
                self.assertFalse(scope[output])

    def test_specification_with_rust_runs_layout_in_the_regular_suite(self):
        scope = select(ROOT, ['docs/formats/f3d.md', 'crates/cadmpeg-codec-f3d/src/lib.rs'])
        self.assertTrue(scope['rust'])
        self.assertFalse(scope['layout'])
        self.assertIn('cadmpeg', scope['packages'])

    def test_corpus_manifest_selects_its_cli_test_without_full_gate(self):
        scope = select(ROOT, ['corpus/manifest.toml'])
        self.assertEqual(scope['packages'], ['cadmpeg'])
        self.assertFalse(scope['full'])
        self.assertFalse(scope['fuzz'])
        self.assertFalse(scope['schema'])
        self.assertFalse(scope['step_cli'])
        self.assertFalse(scope['sldprt_cli'])

    def test_specification_with_unrelated_package_keeps_layout_gate(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'Cargo.toml').write_text('[workspace]\nmembers=["crates/*"]\n')
            package = root / 'crates' / 'independent'
            package.mkdir(parents=True)
            (package / 'Cargo.toml').write_text('[package]\nname="independent"\n')
            scope = select(root, ['docs/formats/f3d.md', 'crates/independent/src/lib.rs'])
            self.assertTrue(scope['rust'])
            self.assertTrue(scope['layout'])

    def test_fuzz_follows_the_fuzz_crate_dependencies(self):
        self.assertTrue(select(ROOT, ['crates/cadmpeg-codec-f3d/src/lib.rs'])['fuzz'])
        self.assertFalse(select(ROOT, ['crates/cadmpeg-registry/src/lib.rs'])['fuzz'])


if __name__ == '__main__':
    unittest.main()
