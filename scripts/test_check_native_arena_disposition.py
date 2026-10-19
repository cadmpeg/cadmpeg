#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Production inventory extraction and drift-check regressions."""

from contextlib import redirect_stderr
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('check-native-arena-disposition.py')
SPEC = importlib.util.spec_from_file_location('native_arena_check', SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


class NativeArenaDispositionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'docs').mkdir()
        for registry in checker.REGISTRIES:
            path = self.root / registry.source
            path.parent.mkdir(parents=True, exist_ok=True)
            if registry.kind == 'macro':
                text = f'{registry.selector}! {{ stored_row: Record {{ pub(crate) stored; }}, child_row: Child {{ => owner.children; }}, schema_only: Helper {{}} }}'
            elif registry.kind == 'enum':
                text = f'impl {registry.selector} {{ const fn as_str(self) -> &\'static str {{ match self {{ Self::Only => "registered_row" }} }} }}'
            elif registry.kind == 'catalogue':
                text = f'const {registry.selector}: &[Row] = &[Row {{ arena: "registered_row", emit: |ctx| ctx.emit() }}];'
            else:
                text = f'const {registry.selector}: &[&str] = &["registered_row"];'
            path.write_text(text)

    def test_all_production_registry_forms_render(self):
        document = checker.render(self.root)
        for registry in checker.REGISTRIES:
            self.assertIn(f'| `{registry.namespace}` |', document)
            self.assertIn(registry.source, document)
        catia = checker.read_registry(self.root, checker.REGISTRIES[0])
        self.assertEqual(catia, ['child_row', 'stored_row'])
        self.assertNotIn('`schema_only`', document)

    def test_registry_change_requires_document_update(self):
        self.assertEqual(checker.main(['--root', str(self.root), '--write']), 0)
        self.assertEqual(checker.main(['--root', str(self.root)]), 0)
        registry = next(row for row in checker.REGISTRIES if row.namespace == 'rhino')
        source = self.root / registry.source
        source.write_text(source.read_text().replace('"registered_row"', '"registered_row", "new_row"'))
        with redirect_stderr(io.StringIO()) as output:
            self.assertEqual(checker.main(['--root', str(self.root)]), 1)
        self.assertIn('new_row', output.getvalue())
        self.assertEqual(checker.main(['--root', str(self.root), '--write']), 0)
        self.assertEqual(checker.main(['--root', str(self.root)]), 0)

    def test_test_literals_and_comments_do_not_add_arenas(self):
        registry = next(row for row in checker.REGISTRIES if row.namespace == 'f3d')
        source = self.root / registry.source
        source.write_text(source.read_text() + '\n// "comment_only"\n#[cfg(test)] fn test_fixture() { let value = "test_only"; }')
        self.assertEqual(checker.read_registry(self.root, registry), ['registered_row'])

    def test_duplicate_registry_name_is_structural_error(self):
        registry = next(row for row in checker.REGISTRIES if row.namespace == 'fcstd')
        source = self.root / registry.source
        source.write_text(source.read_text().replace('"registered_row"', '"registered_row", "registered_row"'))
        with self.assertRaisesRegex(checker.RegistryError, 'duplicate'):
            checker.read_registry(self.root, registry)

    def test_arena_constants_require_literal_names(self):
        registry = next(row for row in checker.REGISTRIES if row.namespace == 'step')
        source = self.root / registry.source
        source.write_text(source.read_text().replace('"registered_row"', 'IMPORTED_NAMES'))
        with self.assertRaisesRegex(checker.RegistryError, 'literal'):
            checker.read_registry(self.root, registry)

    def test_hand_edited_policy_text_is_checked_too(self):
        self.assertEqual(checker.main(['--root', str(self.root), '--write']), 0)
        document = self.root / checker.DOCUMENT
        document.write_text(document.read_text().replace('source fidelity', 'hidden source bytes'))
        with redirect_stderr(io.StringIO()):
            self.assertEqual(checker.main(['--root', str(self.root)]), 1)


if __name__ == '__main__':
    unittest.main()
