#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that CI selection retains dependents and conservatively handles inputs."""

from pathlib import Path
import unittest

from scripts.ci_scope import select
from scripts.ci_cargo import command

ROOT = Path(__file__).resolve().parent.parent


class ScopeTests(unittest.TestCase):
    def test_cargo_command_preserves_arguments(self):
        self.assertEqual(command(['test', '--doc'], ['cadmpeg-core', 'cadmpeg-ir']),
                         ['cargo', 'test', '-q', '-p', 'cadmpeg-core',
                          '-p', 'cadmpeg-ir', '--doc'])
        with self.assertRaises(ValueError):
            command(['test'], [])

    def test_bounded_gate_tool_change_selects_iges(self):
        scope = select(ROOT, ['scripts/verify-iges-bounded.py'])
        self.assertIn('cadmpeg-codec-iges', scope['packages'])
        self.assertTrue(scope['iges'])

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
