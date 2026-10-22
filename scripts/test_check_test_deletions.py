#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Real-Git tests of the two-baseline census, moves, renames and citations."""

import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('check-test-deletions.py')
SPEC = importlib.util.spec_from_file_location('test_deletion_check', SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


class CensusTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'pcurve')
        self.git('config', 'user.email', '301495646+pcurve@users.noreply.github.com')
        self.path = self.root / 'crates/cadmpeg-codec-sldprt/src/lib.rs'
        self.path.parent.mkdir(parents=True)

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.root, stderr=subprocess.STDOUT, text=True).strip()

    def commit(self, message):
        self.git('add', '.')
        self.git('commit', '-q', '-m', message)
        return self.git('rev-parse', 'HEAD')

    def baseline(self, source):
        self.path.write_text(source)
        base = self.commit('base')
        self.git('update-ref', 'refs/remotes/origin/main', base)
        self.git('tag', checker.CHECKPOINT)

    def test_missing_function_reports_deleting_commit_and_exact_section(self):
        self.baseline('#[test]\nfn old_check() { assert!(true); }\n')
        self.path.write_text('')
        deleted = self.commit('Remove reversed behavior (#209 §2.4)')
        result = checker.census(self.root)
        self.assertEqual(len(result['missing_tests']), 1)
        row = result['missing_tests'][0]
        self.assertEqual(row['test'], 'old_check')
        self.assertEqual(row['deletion']['commit'], deleted)
        self.assertEqual(row['deletion']['section_citations'], ['#209 §2.4'])
        self.assertTrue(row['behavior_review_required'])
        self.assertEqual(result['uncited_deletions'], 0)

    def test_vague_issue_reference_does_not_count_as_section_citation(self):
        self.baseline('#[test]\nfn old_check() {}\n')
        self.path.write_text('')
        self.commit('Remove old check for #209')
        result = checker.census(self.root)
        self.assertEqual(result['uncited_deletions'], 1)

    def test_pure_test_move_keeps_name_without_citation(self):
        self.baseline('#[test]\nfn moved() {}\n')
        target = self.path.with_name('tests.rs')
        target.write_text(self.path.read_text())
        self.path.write_text('mod tests;\n')
        self.commit('Move test to owner')
        self.assertEqual(checker.census(self.root)['missing_tests'], [])

    def test_only_five_explicit_renames_are_accepted(self):
        old = 'unrepresented_source_synthesizes_without_patch_provenance'
        new = checker.RENAMES[('cadmpeg-codec-sldprt', old)]
        self.baseline(f'#[test]\nfn {old}() {{}}\n#[test]\nfn arbitrary_source() {{}}\n')
        self.path.write_text(f'#[test]\nfn {new}() {{}}\n#[test]\nfn arbitrary_classes() {{}}\n')
        self.commit('Rename functions')
        result = checker.census(self.root)
        self.assertEqual([row['test'] for row in result['missing_tests']], ['arbitrary_source'])

    def test_checkpoint_test_is_required_even_when_absent_at_merge_base(self):
        self.baseline('')
        self.git('tag', '-d', checker.CHECKPOINT)
        self.path.write_text('#[test]\nfn checkpoint_only() {}\n')
        self.commit('checkpoint')
        self.git('tag', checker.CHECKPOINT)
        self.path.write_text('')
        self.commit('delete checkpoint-only check')
        self.assertEqual([row['test'] for row in checker.census(self.root)['missing_tests']], ['checkpoint_only'])

    def test_rust_strings_and_comments_do_not_declare_tests(self):
        self.baseline('let example = r#"#[test] fn string_only() {}"#;\n// #[test] fn comment_only() {}\n#[test]\nfn actual() {}\n')
        self.assertEqual([test.name for test in checker.inventory(self.root, 'HEAD')], ['actual'])

    def test_nested_rust_comments_do_not_declare_tests(self):
        self.baseline('/* outer /* inner */ #[test] fn comment_only() {} */\n#[test] fn actual() {}\n')
        self.assertEqual([test.name for test in checker.inventory(self.root, 'HEAD')], ['actual'])

    def test_macro_generated_names_are_counted_and_can_be_deleted(self):
        definition = 'macro_rules! make_test { ($name:ident) => { #[test] fn $name() {} }; }\n'
        self.baseline(definition + 'make_test!(first); make_test!(second);\n')
        self.path.write_text(definition + 'make_test!(first);\n')
        deleted = self.commit('Remove second macro invocation (#212 Rule B)')
        self.assertEqual([test.name for test in checker.inventory(self.root, 'HEAD')], ['first'])
        result = checker.census(self.root)
        self.assertEqual([row['test'] for row in result['missing_tests']], ['second'])
        self.assertEqual(result['missing_tests'][0]['deletion']['commit'], deleted)
        self.assertEqual(result['missing_tests'][0]['deletion']['section_citations'], ['#212 Rule B'])

    def test_removing_only_test_attribute_reports_deleting_commit(self):
        self.baseline('#[test]\nfn former_test() {}\n')
        self.path.write_text('fn former_test() {}\n')
        deleted = self.commit('Remove test attribute (#209 §2.4)')
        row = checker.census(self.root)['missing_tests'][0]
        self.assertEqual(row['deletion']['commit'], deleted)
        self.assertEqual(row['deletion']['section_citations'], ['#209 §2.4'])

    def test_unresolved_macro_binding_fails_loudly(self):
        self.baseline('macro_rules! tests { ($( $name:ident ),*) => { $( #[test] fn $name() {} )* }; }\ntests!(one,two);\n')
        with self.assertRaisesRegex(checker.CensusError, 'explicit test-name expansion'):
            checker.inventory(self.root, 'HEAD')

    def test_duplicate_function_names_keep_required_multiplicity(self):
        self.baseline('mod one { #[test] fn check() {} }\nmod two { #[test] fn check() {} }\n')
        self.path.write_text('mod one { #[test] fn check() {} }\n')
        self.commit('Drop one copy')
        row = checker.census(self.root)['missing_tests'][0]
        self.assertEqual((row['test'], row['missing_count']), ('check', 1))

    def test_direct_deletion_needs_one_history_stream_and_no_owner_snapshots(self):
        self.baseline('#[test] fn removed() {}\n')
        self.path.write_text('')
        self.commit('Remove old behavior (#209 §2.4)')
        result = checker.census(self.root)
        self.assertEqual(result['metrics']['history_commands'], 1)
        self.assertEqual(result['metrics']['historical_blob_reader_processes'], 1)
        self.assertEqual(result['metrics']['owner_snapshot_reconstructions'], 0)
        self.assertEqual(result['uncited_deletions'], 0)

    def test_move_then_delete_reports_deletion_instead_of_move(self):
        self.baseline('#[test] fn moved() {}\n')
        target = self.path.with_name('tests.rs')
        target.write_text(self.path.read_text())
        self.path.write_text('mod tests;\n')
        self.commit('Pure test move')
        target.write_text('')
        deleted = self.commit('Remove obsolete behavior (#212 Rule B)')
        row = checker.census(self.root)['missing_tests'][0]
        self.assertEqual(row['deletion']['commit'], deleted)
        self.assertEqual(row['unresolved_deletion_count'], 0)

    def test_authorized_rename_then_delete_reports_actual_later_deletion(self):
        old = 'unrepresented_source_synthesizes_without_patch_provenance'
        new = checker.RENAMES[('cadmpeg-codec-sldprt', old)]
        self.baseline(f'#[test] fn {old}() {{}}\n')
        self.path.write_text(f'#[test] fn {new}() {{}}\n')
        self.commit('Authorized rename')
        self.git('tag', '-f', checker.CHECKPOINT)
        self.path.write_text('')
        deleted = self.commit('Remove behavior (#209 §2.4)')
        rows = checker.census(self.root)['missing_tests']
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['source_names'], sorted([old, new]))
        self.assertEqual(rows[0]['deletion']['commit'], deleted)

    def test_two_same_name_removals_require_both_deleting_commits(self):
        self.baseline('mod one { #[test] fn check() {} }\nmod two { #[test] fn check() {} }\n')
        self.path.write_text('mod one { #[test] fn check() {} }\n')
        first = self.commit('Remove one obsolete behavior (#209 §2.4)')
        self.path.write_text('')
        second = self.commit('Remove other obsolete behavior (#212 Rule B)')
        row = checker.census(self.root)['missing_tests'][0]
        self.assertEqual(row['missing_count'], 2)
        self.assertEqual([event['commit'] for event in row['deletions']], [second, first])
        self.assertEqual(row['unresolved_deletion_count'], 0)

    def test_external_test_macro_uses_only_ambiguous_owner_snapshots(self):
        target = self.path.with_name('tests.rs')
        target.write_text('make_test!(first); make_test!(second);\n')
        self.baseline('macro_rules! make_test { ($name:ident) => { #[test] fn $name() {} }; }\n')
        target.write_text('make_test!(first);\n')
        deleted = self.commit('Remove expanded test (#212 Rule B)')
        result = checker.census(self.root)
        self.assertEqual(result['missing_tests'][0]['deletion']['commit'], deleted)
        self.assertEqual(result['metrics']['owner_snapshot_reconstructions'], 2)

    def test_missing_checkpoint_fails_instead_of_dropping_that_baseline(self):
        self.baseline('#[test] fn actual() {}\n')
        self.git('tag', '-d', checker.CHECKPOINT)
        with self.assertRaises(checker.CensusError):
            checker.census(self.root)

    def test_explicit_word_section_and_named_rules_are_real_citations(self):
        self.assertEqual(checker.citations('#209 section 5.2; #230 section 8'),
                         ['#209 section 5.2', '#230 section 8'])
        self.assertEqual(checker.citations('section 2.4 (#212)'), ['section 2.4 (#212)'])
        self.assertEqual(checker.citations('#209 D3; #209 E1; #209 S-entity'),
                         ['#209 D3', '#209 E1', '#209 S-entity'])
        self.assertEqual(checker.citations('See #209 and #212'), [])

    def test_merge_records_actual_first_parent_removal_without_borrowing_citation(self):
        self.baseline('#[test] fn original() {}\n')
        trunk = self.git('branch', '--show-current')
        self.git('checkout', '-q', '-b', 'topic')
        self.path.write_text('')
        self.commit('Remove original behavior (#209 §2.4)')
        self.git('checkout', '-q', trunk)
        (self.root / 'unrelated').write_text('independent trunk change')
        self.commit('Independent change')
        self.git('merge', '-q', '--no-ff', '-m', 'Merge topic', 'topic')
        merged = self.git('rev-parse', 'HEAD')
        result = checker.census(self.root)
        self.assertEqual(result['missing_tests'][0]['deletion']['commit'], merged)
        self.assertEqual(result['missing_tests'][0]['deletion']['section_citations'], [])
        self.assertEqual(result['uncited_deletions'], 1)


if __name__ == '__main__':
    unittest.main()
