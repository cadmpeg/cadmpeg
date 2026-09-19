#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-source-policy.py``."""

from __future__ import annotations

import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-source-policy.py")
SPEC = importlib.util.spec_from_file_location("source_policy", SCRIPT)
assert SPEC and SPEC.loader
policy = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = policy
SPEC.loader.exec_module(policy)


def _pad_lines(prefix: list[str], total_lines: int, *, trailing_newline: bool = True) -> str:
    assert total_lines >= len(prefix)
    lines = prefix + [f"// filler {i}" for i in range(total_lines - len(prefix))]
    text = "\n".join(lines)
    if trailing_newline:
        return text + "\n"
    return text


class TempSourceCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.old_root = policy.ROOT
        policy.ROOT = self.root

    def tearDown(self) -> None:
        policy.ROOT = self.old_root
        self._tmp.cleanup()

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def findings(self, rule: str) -> list[policy.Finding]:
        return [item for item in policy.check_source() if item.rule == rule]


class StripCfgTest(unittest.TestCase):
    def test_long_flat_cfg_does_not_backtrack(self) -> None:
        # A subprocess deadline also bounds failures if the old regex returns.
        probe = """
import runpy, sys
classify = runpy.run_path(sys.argv[1])["attr_is_test_cfg"]
prefix = "#[cfg(all(" + " ," * 10000
assert not classify(prefix + "))]")
assert classify(prefix + "test))]")
assert not classify('#[cfg(all(any(test, feature = "x")))]')
"""
        subprocess.run(
            [sys.executable, "-c", probe, str(SCRIPT)],
            check=True, timeout=5, capture_output=True, text=True,
        )

    def test_strips_cfg_test_mod_body(self) -> None:
        text = (
            "fn prod() { from_le_bytes(); }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn t() { from_le_bytes(); }\n"
            "}\n"
            "fn other() { from_be_bytes(); }\n"
        )
        stripped, _ = policy.production_source(text)
        self.assertEqual(
            policy.FROM_ENDIAN.findall(stripped),
            ["from_le_bytes", "from_be_bytes"],
        )

    def test_strips_cfg_test_fn_whose_signature_holds_an_array_type(self) -> None:
        text = (
            "fn prod() { from_le_bytes(); }\n"
            "#[cfg(test)]\n"
            "fn helper(sizes: Option<[f64; 3]>) -> u8 {\n"
            "    from_be_bytes();\n"
            "    0\n"
            "}\n"
            "fn other() { from_le_bytes(); }\n"
        )
        stripped, _ = policy.production_source(text)
        self.assertEqual(
            policy.FROM_ENDIAN.findall(stripped),
            ["from_le_bytes", "from_le_bytes"],
        )

    def test_same_line_item_boundaries_preserve_locations_and_line_counts(self) -> None:
        text = '#[test] fn fixture() {} fn production() { from_le_bytes(); }\n'
        code, count = policy.production_source(text)
        self.assertEqual(len(code), len(text))
        self.assertEqual(count, 1)
        self.assertNotIn("fixture", code)
        self.assertEqual(code.index("from_le_bytes"), text.index("from_le_bytes"))
        self.assertEqual(policy.production_source('#[test] fn fixture() {}\n')[1], 0)

    def test_incomplete_test_item_is_retained_conservatively(self) -> None:
        text = '#[test] fn fixture() { from_le_bytes();\n'
        code, count = policy.production_source(text)
        self.assertIn("from_le_bytes", code)
        self.assertEqual(count, 1)

    def test_keeps_non_test_cfg(self) -> None:
        text = "#[cfg(feature = \"x\")]\nfn f() { from_le_bytes(); }\n"
        stripped, _ = policy.production_source(text)
        self.assertEqual(policy.FROM_ENDIAN.findall(stripped), ["from_le_bytes"])

    def test_masks_cfg_test_items_and_counts_only_production_lines(self) -> None:
        text = (
            "fn prod() {}\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn t() {}\n"
            "}\n"
            "fn other() {}\n"
        )
        code, count = policy.production_source(text)
        self.assertEqual(count, 2)
        self.assertEqual(len(code), len(text))
        self.assertEqual(code.splitlines()[0], "fn prod() {}")
        self.assertEqual(code.splitlines()[5], "fn other() {}")
        self.assertTrue(all(not line.strip() for line in code.splitlines()[1:5]))

    def test_cfg_classification_keeps_production_and_masks_test_only_items(self) -> None:
        for expression, test_only in [
            ("test", True), ('all(test, feature = "schema")', True),
            ('all(feature = "schema", test)', True),
            ("not(test)", False), ('any(feature = "examples", test)', False),
            ('feature = "test"', False), ("not(not(test))", False),
        ]:
            with self.subTest(expression=expression):
                source = f"#[cfg({expression})]\nfn f() {{ from_le_bytes(); }}\n"
                code, count = policy.production_source(source)
                self.assertEqual(count, 0 if test_only else 2)
                self.assertEqual("from_le_bytes" in code, not test_only)
                self.assertEqual(policy.attr_is_test_cfg(f"#[cfg({expression})]"), test_only)

    def test_multiline_attributes_and_literal_braces_preserve_locations(self) -> None:
        source = (
            "#[cfg(\n test\n)]\n#[allow(dead_code)]\n"
            'fn test_only() { let s = "}"; }\n'
            'fn prod() { let s = "#[cfg(test)]"; }\n'
            "fn other() { from_le_bytes(); }"
        )
        code, count = policy.production_source(source)
        self.assertEqual(count, 2)
        self.assertEqual(len(code), len(source))
        findings = policy.scan_patterns(policy.ROOT / "source.rs", source)
        self.assertEqual([(f.rule, f.line) for f in findings], [("unapproved_endian_read", 7)])


class PatternFilters(unittest.TestCase):
    def test_lifetimes_and_loop_labels_do_not_hide_code(self) -> None:
        text = "'outer: loop { from_le_bytes(); break 'outer; }\n"
        self.assertEqual(policy.mask_rust_non_code(text), text)

    def test_nested_comments_are_one_non_code_span(self) -> None:
        text = '/* outer /* inner */ from_le_bytes(); */ from_be_bytes();'
        self.assertEqual(
            policy.FROM_ENDIAN.findall(policy.mask_rust_non_code(text)),
            ["from_be_bytes"],
        )

    def test_raw_string_backslash_does_not_escape_its_end(self) -> None:
        text = 'let s = r"backslash\\"; from_le_bytes();'
        self.assertEqual(
            policy.FROM_ENDIAN.findall(policy.mask_rust_non_code(text)),
            ["from_le_bytes"],
        )

    def test_literals_keep_positions_and_do_not_consume_following_code(self) -> None:
        for literal in [
            "'a'", "'é'", r"'\u{1f600}'", r"'\x41'", r"'\''", "b'a'",
            '"from_le_bytes()\\\""', 'r##"from_le_bytes() \\"#"##',
            'br"from_le_bytes()\\"', 'cr#"from_le_bytes()"#',
        ]:
            with self.subTest(literal=literal):
                text = literal + ';\nfrom_be_bytes();'
                masked = policy.mask_rust_non_code(text)
                self.assertEqual(len(masked), len(text))
                self.assertEqual(masked.count('\n'), text.count('\n'))
                self.assertEqual(policy.FROM_ENDIAN.findall(masked), ["from_be_bytes"])

    def test_test_support_is_not_production_source(self) -> None:
        self.assertFalse(
            policy.is_production_rs(
                Path("crates/codec/src/test_support/fixture_builder.rs")
            )
        )

    def test_excludes_loss_note_return_and_struct(self) -> None:
        text = (
            "struct LossNote {\n"
            "    msg: String,\n"
            "}\n"
            "impl LossNote {\n"
            "    fn new() -> Self { todo!() }\n"
            "}\n"
            "fn make() -> LossNote {\n"
            "    LossNote { msg: String::new() }\n"
            "}\n"
        )
        hits = policy.scan_patterns(policy.ROOT / "source.rs", text)
        self.assertEqual([item.rule for item in hits], ["loss_note_literal"])

    def test_malformed_format_multiline(self) -> None:
        text = 'CodecError::Malformed(format!(\n    "bad {}", x\n))\n'
        self.assertEqual(len(policy.MALFORMED_FORMAT.findall(text)), 1)

    def test_loss_note_qualified_returns_are_types(self) -> None:
        for name in ["LossNote", "cadmpeg_ir::LossNote", "::cadmpeg_ir::report::LossNote",
                     "r#type::LossNote", "données::LossNote", "r#LossNote"]:
            for gap in [" ", "\n    ", " /* return type */ "]:
                with self.subTest(name=name, gap=gap):
                    text = f"fn make() ->{gap}{name}\n{{ code.note(message) }}"
                    self.assertEqual(policy.scan_patterns(policy.ROOT / "source.rs", text), [])

    def test_loss_note_type_occurrence_does_not_hide_inline_literals(self) -> None:
        for text, expected in [
            ("fn make() -> LossNote { LossNote { message } }", [1]),
            ("fn make() -> cadmpeg_ir::LossNote { cadmpeg_ir::LossNote { message } }", [1]),
            ("struct LossNote {} fn make() { LossNote { message }; LossNote { message }; }", [1, 1]),
            ("impl LossNote { fn make() -> Self { LossNote { message } } }", [1]),
            ("impl Trait for cadmpeg_ir::LossNote { fn make() { LossNote { message }; } }", [1]),
            ("struct r#LossNote {} impl r#LossNote {} impl Trait for r#LossNote {}", []),
            ("fn make() ->\n LossNote {\n LossNote { message }\n}", [3]),
        ]:
            with self.subTest(text=text):
                hits = policy.scan_patterns(policy.ROOT / "source.rs", text)
                self.assertEqual([(hit.rule, hit.line) for hit in hits], [("loss_note_literal", line) for line in expected])

    def test_loss_note_rule_matches_the_complete_type_name(self) -> None:
        text = "fn make() { OtherLossNote { message }; LossNoteSuffix { message }; }"
        self.assertEqual(policy.scan_patterns(policy.ROOT / "source.rs", text), [])

    def test_production_filter_rejects_test_paths(self) -> None:
        self.assertFalse(policy.is_production_rs(Path("crates/c/src/foo_test.rs")))
        self.assertFalse(policy.is_production_rs(Path("crates/c/tests/foo.rs")))
        self.assertFalse(policy.is_production_rs(Path("crates/c/src/tests.rs")))
        self.assertTrue(policy.is_production_rs(Path("crates/c/src/decode.rs")))

    def test_bare_tolerance_includes_seven_eight_eleven(self) -> None:
        text = (
            "a 1e-6 b 1e-7 c 1e-8 d 1e-9 e 1e-10 f 1e-11 g 1e-12 "
            "h 1e-18 i 1.0e-9 j 1.00E-10\n"
        )
        self.assertEqual(
            policy.BARE_TOLERANCE.findall(text),
            [
                "1e-6",
                "1e-7",
                "1e-8",
                "1e-9",
                "1e-10",
                "1e-11",
                "1e-12",
                "1.0e-9",
                "1.00E-10",
            ],
        )

    def test_source_masks_comments_and_literals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "source.rs"
            path.write_text(
                '// from_le_bytes 1e-9\nconst NOTE: &str = "from_be_bytes 1e-8";\n'
                "fn actual() { from_le_bytes(); let _ = 1e-7; }\n",
                encoding="utf-8",
            )
            source, _ = policy.production_source(path.read_text())
        self.assertEqual(policy.FROM_ENDIAN.findall(source), ["from_le_bytes"])
        self.assertEqual(len([item for item in policy.scan_patterns(policy.ROOT / "source.rs", source) if item.rule == "bare_tolerance"]), 1)

    def test_bare_tolerance_excludes_named_threshold_initializers(self) -> None:
        text = (
            "const EPS_DIRECT: f64 = 1e-9;\n"
            "pub(crate) static EPS_STATIC: f64 = 1.0e-10;\n"
            "let direct = 1e-9;\n"
            "let formatted = 1.0e-10;\n"
            "const DERIVED: f64 = f64::from_bits(1e-9 as u64);\n"
            "const MULTILINE: f64 =\n"
            "    1.0e-11;\n"
        )
        self.assertEqual(len([item for item in policy.scan_patterns(policy.ROOT / "source.rs", text) if item.rule == "bare_tolerance"]), 3)

    def test_vec_repeat_scanner_ignores_nested_delimiters_and_strings(self) -> None:
        text = r'''
            let bytes = vec![0u8; len];
            let nested = vec![[0; 2]; outer_len];
            let message = vec![format!("a; b")];
            // vec![0; commented_len]
        '''
        self.assertEqual(
            [count for _, count in policy.iter_vec_repeats(policy.mask_rust_non_code(text))],
            ["len", "outer_len"],
        )

    def test_vec_patterns_use_masked_source_and_report_nested_repeats(self) -> None:
        source = '\n'.join([
            'let text = r#"vec![0; hidden]; ]"#;',
            "let nested = vec![vec![0; inner]; outer];",
            'let quoted = vec!["]; /*"; count];',
            "/* vec![0; hidden] */ let safe = vec![0; 4];",
        ])
        hits = policy.scan_patterns(policy.ROOT / "source.rs", source)
        self.assertEqual([(f.rule, f.line) for f in hits], [
            ("unchecked_vec_repeat", 2), ("unchecked_vec_repeat", 2),
            ("unchecked_vec_repeat", 3),
        ])

    def test_existing_collection_lengths_are_admitted_repeat_sizes(self) -> None:
        self.assertIsNotNone(policy.ADMITTED_LEN_REPEAT.fullmatch("records.len()"))
        self.assertIsNotNone(
            policy.ADMITTED_LEN_REPEAT.fullmatch("self.records.len() + 1")
        )
        self.assertIsNone(policy.ADMITTED_LEN_REPEAT.fullmatch("parsed_count"))

    def test_from_endian_counts_non_codec_crates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for relative, source in (
                (
                    "crates/cadmpeg-codec-demo/src/lib.rs",
                    "fn f() { u32::from_le_bytes([0; 4]); }\n",
                ),
                (
                    "crates/cadmpeg-protein/src/lib.rs",
                    "fn f() { u32::from_le_bytes([0; 4]); u64::from_be_bytes([0; 8]); }\n",
                ),
            ):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            old_root = policy.ROOT
            try:
                policy.ROOT = root
                self.assertEqual(len([item for item in policy.check_source() if item.rule == "unapproved_endian_read"]), 3)
            finally:
                policy.ROOT = old_root



class PlacementRules(TempSourceCase):
    def test_production_cfg_is_not_test_placement_or_excluded_from_size(self) -> None:
        self.write("crates/demo/src/lib.rs", '\n'.join([
            '#[cfg(not(test))]', '#[path = "prod.rs"]', 'mod prod;',
            '#[cfg(any(feature = "examples", test))]',
            '#[path = "examples.rs"]', 'mod examples;', '',
        ]))
        self.write("crates/demo/src/prod.rs",
                   "#[cfg(not(test))]\nfn prod() {\n    work();\n}\n")
        self.write("crates/demo/src/examples.rs", "fn example() {}\n")
        self.assertEqual(self.findings("test_path_include"), [])
        with patch.object(policy, "PRODUCTION_LINE_LIMIT", 3):
            oversized = self.findings("production_size")
        self.assertIn("crates/demo/src/prod.rs", [f.path for f in oversized])

    def test_root_router_but_not_nested_tests(self) -> None:
        self.write("crates/demo/src/lib.rs", "#[cfg(test)]\nmod tests;\nmod foo;\n")
        self.write("crates/demo/src/foo.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("crates/demo/src/tests.rs", _pad_lines(["mod router_only;"], 2001))
        self.write("crates/demo/src/foo/tests.rs", _pad_lines(["fn helper() {}"], 2001))
        self.assertEqual([f.path for f in self.findings("crate_root_tests")], ["crates/demo/src/tests.rs"])
        self.assertEqual({f.path for f in self.findings("test_size")}, {
            "crates/demo/src/tests.rs", "crates/demo/src/foo/tests.rs",
        })

    def test_path_includes_follow_test_only_ancestry(self) -> None:
        self.write("crates/demo/src/lib.rs", "")
        self.write("crates/demo/src/tests.rs", '\n'.join([
            '#[path = "integration_tests.rs"]', 'mod integration_tests;',
            'mod nested {', '    #[path =', '        "bytes.rs"', '    ]',
            '    mod bytes;', '}', '',
        ]))
        self.write("crates/demo/src/integration_tests.rs", "fn smoke() {}\n")
        self.write("crates/demo/src/bytes.rs", _pad_lines(["fn helper() {}"], 2001))
        includes = self.findings("test_path_include")
        self.assertEqual([(f.path, f.line) for f in includes], [
            ("crates/demo/src/tests.rs", 1), ("crates/demo/src/tests.rs", 4),
        ])
        self.assertIn("integration_tests.rs", includes[0].message)
        self.assertIn("bytes.rs", includes[1].message)
        self.assertEqual([f.path for f in self.findings("test_size")], ["crates/demo/src/bytes.rs"])

    def test_feature_gated_non_test_path_is_allowed(self) -> None:
        self.write("crates/demo/src/lib.rs", '\n'.join([
            '#[cfg(feature = "fuzzing")]', '#[path = "fuzzing.rs"]', 'pub mod fuzz;',
            '#[cfg(test)]', '#[path = "unit.rs"]', 'mod tests;', '',
        ]))
        self.write("crates/demo/src/fuzzing.rs", "pub fn fuzz() {}\n")
        self.write("crates/demo/src/unit.rs", "fn helper() {}\n")
        findings = self.findings("test_path_include")
        self.assertEqual([(f.path, f.line) for f in findings], [("crates/demo/src/lib.rs", 4)])
        self.assertIn("unit.rs", findings[0].message)

    def test_semantic_dirs_integration_support_and_golden_exclusion(self) -> None:
        self.write("crates/demo/src/lib.rs", "#[cfg(test)]\nmod golden_tests;\nmod foo;\n")
        self.write("crates/demo/src/foo.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("crates/demo/src/foo/tests.rs", "mod parsing;\n")
        paths = ["crates/demo/src/foo/tests/parsing.rs",
                 "crates/demo/src/integration_tests.rs", "crates/demo/src/test_support.rs"]
        for path in paths:
            self.write(path, _pad_lines(["fn helper() {}"], 2001))
        self.write("crates/demo/src/golden_tests.rs", _pad_lines(["fn golden() {}"], 2501))
        self.assertEqual({f.path for f in self.findings("test_size")}, set(paths))

    def test_inline_test_module_uses_module_span(self) -> None:
        text = "\n".join(["fn prod() {}", "#[cfg(test)]", "mod tests {"]
                         + [f"    // filler {i}" for i in range(1998)] + ["}"])
        self.write("crates/demo/src/lib.rs", text + "\n")
        findings = self.findings("test_size")
        self.assertEqual([(f.path, f.line) for f in findings], [("crates/demo/src/lib.rs", 2)])
        self.assertEqual(findings[0].message, "Inline test module tests has 2001 lines; limit is 2000.")

    def test_exact_boundaries_and_no_trailing_newline(self) -> None:
        # The boundary is the limit the module states, so the test patches both
        # limits down and states the same two facts over four lines: a file at
        # the limit is clean, and a file one line past it is named whether or
        # not it ends with a newline.
        self.write("crates/demo/src/lib.rs", "mod prod;\n")
        with patch.object(policy, "PRODUCTION_LINE_LIMIT", 4), \
                patch.object(policy, "TEST_LINE_LIMIT", 3):
            self.write("crates/demo/src/prod.rs", _pad_lines(["fn prod() {}"], 4))
            self.write("crates/demo/src/test_support.rs",
                       _pad_lines(["fn helper() {}"], 3, trailing_newline=False))
            self.assertEqual(policy.check_source(), [])
            self.write("crates/demo/src/prod.rs",
                       _pad_lines(["fn prod() {}"], 5, trailing_newline=False))
            self.write("crates/demo/src/test_support.rs",
                       _pad_lines(["fn helper() {}"], 4, trailing_newline=False))
            self.assertEqual([(f.rule, f.path, f.line) for f in policy.check_source()], [
                ("production_size", "crates/demo/src/prod.rs", 1),
                ("test_size", "crates/demo/src/test_support.rs", 1),
            ])

    def test_split_files_are_still_checked(self) -> None:
        self.write("crates/demo/src/lib.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("crates/demo/src/tests.rs", _pad_lines(["fn all_in_one() {}"], 5000))
        self.assertEqual(len(self.findings("test_size")), 1)
        self.write("crates/demo/src/tests.rs", '\n'.join([
            '#[path = "part_a.rs"]', 'mod part_a;',
            '#[path = "part_b.rs"]', 'mod part_b;', '',
        ]))
        self.write("crates/demo/src/part_a.rs", _pad_lines(["fn a() {}"], 2500))
        self.write("crates/demo/src/part_b.rs", _pad_lines(["fn b() {}"], 2500))
        self.assertEqual({f.path for f in self.findings("test_size")}, {
            "crates/demo/src/part_a.rs", "crates/demo/src/part_b.rs",
        })

    def test_cargo_tests_are_checked(self) -> None:
        self.write("crates/demo/src/lib.rs", "")
        self.write("crates/demo/tests/smoke.rs", _pad_lines(["fn smoke() {}"], 2001))
        self.assertEqual([f.path for f in self.findings("test_size")], ["crates/demo/tests/smoke.rs"])


class ModuleVisibility(TempSourceCase):
    def test_private_inner_module_cannot_grant_crate_reach(self) -> None:
        self.write("crates/demo/src/lib.rs", "pub mod outer;\n")
        self.write("crates/demo/src/outer.rs", "mod inner;\n")
        self.write("crates/demo/src/outer/inner.rs", '\n'.join([
            "pub(crate) fn wide() {}", "pub(crate) const WIDE: u8 = 1;",
            "pub(crate) static ALSO: u8 = 1;", "pub(super) fn narrow() {}", "",
        ]))
        findings = self.findings("overwide_module_visibility")
        self.assertEqual([(f.path, f.line) for f in findings], [
            ("crates/demo/src/outer/inner.rs", 1),
            ("crates/demo/src/outer/inner.rs", 2),
            ("crates/demo/src/outer/inner.rs", 3),
        ])
        self.assertIn("outer::inner", findings[0].message)
        self.assertIn("crate::outer", findings[0].message)

    def test_reach_the_declaration_chain_grants_is_accepted(self) -> None:
        # A private module of the crate root: the root's subtree is the whole crate.
        self.write("crates/demo/src/lib.rs", "mod inner;\npub mod outer;\n")
        self.write("crates/demo/src/inner.rs", "pub(crate) fn wide() {}\n")
        # A `pub` chain keeps the parent's reach, and `pub(crate)` widens to it.
        self.write("crates/demo/src/outer.rs", "pub mod shown;\npub(crate) mod lifted;\n")
        self.write("crates/demo/src/outer/shown.rs", "pub(crate) fn wide() {}\n")
        self.write("crates/demo/src/outer/lifted.rs", "pub(crate) fn wide() {}\n")
        self.assertEqual(self.findings("overwide_module_visibility"), [])

    def test_marker_wider_than_a_deeper_cap_is_reported(self) -> None:
        self.write("crates/demo/src/lib.rs", "pub mod a;\n")
        self.write("crates/demo/src/a.rs", "mod b;\n")
        self.write("crates/demo/src/a/b.rs", "mod c;\n")
        self.write("crates/demo/src/a/b/c.rs", '\n'.join([
            "pub(in crate::a) fn wide() {}", "pub(in crate::a::b) fn exact() {}",
            "pub(super) fn narrow() {}", "",
        ]))
        findings = self.findings("overwide_module_visibility")
        self.assertEqual([(f.path, f.line) for f in findings], [
            ("crates/demo/src/a/b/c.rs", 1),
        ])

    def test_forms_the_compiler_can_require_stay_outside_the_rule(self) -> None:
        self.write("crates/demo/src/lib.rs", "pub mod outer;\n")
        self.write("crates/demo/src/outer.rs", "mod inner;\n")
        self.write("crates/demo/src/outer/inner.rs", '\n'.join([
            "pub(crate) struct Held {", "    pub(crate) field: u8,", "}",
            "pub(crate) enum Tag { One }", "pub(crate) type Alias = u8;",
            "impl Held {", "    pub(crate) fn reached(&self) -> u8 {", "        self.field",
            "    }", "}", "#[cfg(test)]", "pub(crate) fn gated() {}", "",
        ]))
        self.assertEqual(self.findings("overwide_module_visibility"), [])

    def test_a_reexported_module_keeps_the_reach_the_reexport_grants(self) -> None:
        self.write("crates/demo/src/lib.rs", "pub mod outer;\n")
        self.write("crates/demo/src/outer.rs", "mod inner;\npub(crate) use inner::wide;\n")
        self.write("crates/demo/src/outer/inner.rs", "pub(crate) fn wide() {}\n")
        self.assertEqual(self.findings("overwide_module_visibility"), [])


class EndianExceptions(TempSourceCase):
    def test_literal_cannot_supply_an_exception(self) -> None:
        self.write("crates/demo/src/lib.rs", '''fn f() {
    let text = r#"
// endian-exception: reconstructed-scalar
"#; f64::from_be_bytes(raw);
}
''')
        self.assertEqual(len(self.findings("unapproved_endian_read")), 1)
        self.assertEqual(self.findings("endian_exception"), [])

    def test_exception_does_not_admit_another_call(self) -> None:
        self.write("crates/demo/src/lib.rs", """fn f() {
    // endian-exception: reconstructed-scalar
    f64::from_be_bytes(raw);
    u32::from_le_bytes(raw);
}
""")
        self.assertEqual([f.line for f in self.findings("unapproved_endian_read")], [4])
        self.assertEqual(self.findings("endian_exception"), [])

    def test_stale_or_unknown_exception_fails(self) -> None:
        for reason, following in [("reconstructed-scalar", "0;"), ("typo", "f64::from_be_bytes(raw);")]:
            self.write("crates/demo/src/lib.rs", f"fn f() {{\n// endian-exception: {reason}\n{following}\n}}\n")
            self.assertEqual(len(self.findings("endian_exception")), 1)


class DiscardedValues(TempSourceCase):
    def test_an_unmarked_discard_is_named(self) -> None:
        self.write("crates/demo/src/lib.rs", "fn f() {\n    let _ = g();\n}\n")
        findings = self.findings("discarded_value")
        self.assertEqual([(f.path, f.line) for f in findings],
                         [("crates/demo/src/lib.rs", 2)])
        self.assertIn("discarded-value", findings[0].message)

    def test_a_typed_discard_is_named(self) -> None:
        self.write("crates/demo/src/lib.rs", "fn f() {\n    let _: u32 = g();\n}\n")
        self.assertEqual([f.line for f in self.findings("discarded_value")], [2])

    def test_a_reason_admits_exactly_one_discard(self) -> None:
        self.write("crates/demo/src/lib.rs", """fn f() {
    // discarded-value: the call's refusal is the whole effect
    let _ = g();
    let _ = h();
}
""")
        self.assertEqual([f.line for f in self.findings("discarded_value")], [4])

    def test_a_stale_or_empty_reason_fails(self) -> None:
        for reason, following in [("the answer has no reader", "0;"), ("", "let _ = g();")]:
            self.write("crates/demo/src/lib.rs",
                       f"fn f() {{\n// discarded-value: {reason}\n    {following}\n}}\n")
            self.assertEqual(len(self.findings("discarded_value")), 1)

    def test_a_literal_cannot_supply_a_reason(self) -> None:
        self.write("crates/demo/src/lib.rs", '''fn f() {
    let text = r#"
// discarded-value: not a reason
"#; let _ = g();
}
''')
        self.assertEqual([f.line for f in self.findings("discarded_value")], [4])

    def test_a_declared_fuzz_entry_point_is_outside_the_rule(self) -> None:
        self.write("crates/demo/src/fuzz.rs", "pub fn run(data: &[u8]) {\n    let _ = g(data);\n}\n")
        self.assertEqual([f.line for f in self.findings("discarded_value")], [2])
        with patch.object(policy, "DISCARD_EXEMPT_FILES",
                          {"crates/demo/src/fuzz.rs": "the fuzzer reads the crash, never the value"}):
            self.assertEqual(self.findings("discarded_value"), [])

    def test_a_cfg_test_discard_is_not_production(self) -> None:
        self.write("crates/demo/src/lib.rs",
                   "#[cfg(test)]\nmod tests {\n    fn t() {\n        let _ = g();\n    }\n}\n")
        self.assertEqual(self.findings("discarded_value"), [])


class ScriptTestCollection(TempSourceCase):
    GUARD = 'if __name__ == "__main__":\n    unittest.main()\n'
    CASE = (
        "import unittest\n\n\n"
        "class ZzScratchTests(unittest.TestCase):\n"
        "    def test_one(self) -> None:\n"
        "        pass\n\n\n"
    )

    def test_a_case_after_the_main_block_is_named(self) -> None:
        self.write("scripts/test_zz_scratch.py", self.GUARD + self.CASE)
        findings = self.findings("script_test_collection")
        self.assertEqual(len(findings), 1, findings)
        self.assertEqual(findings[0].path, "scripts/test_zz_scratch.py")
        self.assertIn("class ZzScratchTests", findings[0].message)
        self.assertIn("test_zz_scratch.py", findings[0].message)

    def test_a_free_test_function_after_the_main_block_is_named(self) -> None:
        self.write(
            "scripts/test_zz_scratch.py",
            self.GUARD + "def test_loose() -> None:\n    pass\n",
        )
        findings = self.findings("script_test_collection")
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("function test_loose", findings[0].message)

    def test_a_case_before_the_main_block_is_collected(self) -> None:
        self.write("scripts/test_zz_scratch.py", self.CASE + self.GUARD)
        self.assertEqual(self.findings("script_test_collection"), [])


class SourcePolicyCommand(TempSourceCase):
    def run_check(self, *args: str) -> tuple[int, str]:
        output = io.StringIO()
        with redirect_stdout(output):
            result = policy.main(list(args))
        return result, output.getvalue()

    def test_clean_source_needs_no_git_or_ledger(self) -> None:
        self.write("crates/demo/src/lib.rs", "fn f() {}\n")
        self.assertEqual(self.run_check(), (0, "source-policy: ok\n"))
        result, output = self.run_check("--json")
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output), {"status": "ok", "findings": []})

    def test_json_and_text_locate_each_pattern(self) -> None:
        self.write("crates/demo/src/lib.rs", """fn f() {
    let x = 1e-9;
    let x = vec![0; count];
    CodecError::Malformed(
        format!("bad {}", x));
    LossNote { code, severity, message, provenance };
}
""")
        result, output = self.run_check("--json")
        self.assertEqual(result, 1)
        findings = json.loads(output)["findings"]
        self.assertEqual([(f["rule"], f["line"]) for f in findings], [
            ("bare_tolerance", 2), ("unchecked_vec_repeat", 3),
            ("formatted_malformed_error", 4), ("loss_note_literal", 6),
        ])
        self.assertTrue(all(f["path"] == "crates/demo/src/lib.rs" and f["message"] for f in findings))
        result, output = self.run_check()
        self.assertEqual(result, 1)
        self.assertIn("crates/demo/src/lib.rs:2: bare_tolerance:", output)

    def test_removed_migration_options_are_rejected(self) -> None:
        for args in (["--base", "HEAD"], ["--update"]):
            with patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit) as error:
                policy.main(args)
            self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
