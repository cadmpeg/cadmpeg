#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-dialects.py``.

Every rule the checker enforces is exercised here by synthesizing a registry
that violates exactly that rule and asserting the matching failure fires. The
last test runs the checker against the committed ``docs/dialects.toml`` and
requires a clean pass.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import re
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-dialects.py")
SPEC = importlib.util.spec_from_file_location("check_dialects", SCRIPT)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)

REPO = SCRIPT.resolve().parent.parent

GOOD_ROW = """
[[dialect]]
id = "demo:one"
title = "Demo one"
discriminants = { marker = "0x01" }
witness = "spec:Demo specification section 1"
"""


def _registry(body: str, *, formats: str = "[format.demo]\ncomplete = true\n") -> str:
    return formats + body


class RegistryCase(unittest.TestCase):
    """Write a synthetic registry into a temporary root and check it."""

    def run_check(self, text: str | None, *, files: list[str] | None = None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            if text is not None:
                (root / "docs" / "dialects.toml").write_text(text, encoding="utf-8")
                ids = re.findall(r'^id = "([^"]+)"$', text, re.MULTILINE)
                source = root / "crates" / "demo" / "src" / "dialect.rs"
                source.parent.mkdir(parents=True)
                source.write_text("\n".join(f'const _: &str = "{value}";' for value in ids), encoding="utf-8")
            for rel in files or []:
                target = root / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("fixture", encoding="utf-8")
            return checker.check(root)

    def assertFires(self, text: str | None, needle: str, *, files: list[str] | None = None):
        failures, _ = self.run_check(text, files=files)
        self.assertTrue(
            any(needle in failure for failure in failures),
            f"expected {needle!r} in {failures}",
        )

    def assertClean(self, text: str, *, files: list[str] | None = None):
        failures, summary = self.run_check(text, files=files)
        self.assertEqual(failures, [], summary)
        return summary


class TestFileLevel(RegistryCase):
    def test_missing_registry(self):
        self.assertFires(None, "not found")

    def test_parse_error(self):
        self.assertFires("this is not = = toml\n", "parse error")

    def test_missing_format_table(self):
        self.assertFires(_registry(GOOD_ROW, formats=""), "no [format.<id>] entries")

    def test_format_not_a_table(self):
        self.assertFires(_registry(GOOD_ROW, formats='format = "demo"\n'), "[format] is not a table")

    def test_format_id_grammar(self):
        self.assertFires(
            _registry(GOOD_ROW, formats='[format."Demo-X"]\ncomplete = true\n'),
            "id must match [a-z0-9]+",
        )

    def test_format_body_not_a_table(self):
        self.assertFires(_registry(GOOD_ROW, formats="format = { demo = 1 }\n"), "not a table")

    def test_format_missing_complete(self):
        self.assertFires(_registry(GOOD_ROW, formats="[format.demo]\n"), "missing complete")

    def test_format_complete_not_boolean(self):
        self.assertFires(
            _registry(GOOD_ROW, formats='[format.demo]\ncomplete = "yes"\n'),
            "complete must be a boolean",
        )

    def test_no_dialect_rows(self):
        self.assertFires(_registry(""), "no [[dialect]] rows")

    def test_dialect_not_an_array_of_tables(self):
        # The assignment precedes the format table: a bare key after
        # `[format.demo]` would land inside it rather than at the top level.
        self.assertFires('dialect = "nope"\n' + _registry(""), "non-empty array of tables")

    def test_dialect_empty_array(self):
        self.assertFires("dialect = []\n" + _registry(""), "non-empty array of tables")

    def test_dialect_element_not_a_table(self):
        self.assertFires('dialect = ["nope"]\n' + _registry(""), "dialect #0: not a table")


class TestRowShape(RegistryCase):
    def test_missing_required_keys(self):
        text = _registry('[[dialect]]\nid = "demo:one"\n')
        failures, _ = self.run_check(text)
        for key in ("title", "discriminants", "witness"):
            self.assertIn(f"demo:one: missing {key}", failures)

    def test_missing_id(self):
        self.assertFires(
            _registry('[[dialect]]\ntitle = "t"\ndiscriminants = { a = "b" }\nwitness = "spec:s"\n'),
            "dialect #0: missing id",
        )

    def test_unknown_key(self):
        self.assertFires(_registry(GOOD_ROW + 'grammar = "chunked"\n'), "unknown key grammar")

    def test_id_not_a_string(self):
        self.assertFires(
            _registry('[[dialect]]\nid = 7\ntitle = "t"\ndiscriminants = { a = "b" }\nwitness = "spec:s"\n'),
            "id must be a string",
        )

    def test_id_shape(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"demo:one"', '"demoone"')),
            "id must be <format>:<name>",
        )

    def test_format_prefix_grammar(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"demo:one"', '"De_mo:one"')),
            "format prefix must match [a-z0-9]+",
        )

    def test_unregistered_format_prefix(self):
        self.assertFires(_registry(GOOD_ROW.replace('"demo:one"', '"other:one"')), "no [format.other] entry")

    def test_name_grammar(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"demo:one"', '"demo:One_Two"')),
            "name must be lowercase [a-z0-9.-]+",
        )

    def test_dots_are_legal_in_a_name(self):
        self.assertClean(_registry(GOOD_ROW.replace('"demo:one"', '"demo:5.3-fixed-ascii"')))

    def test_duplicate_id(self):
        self.assertFires(_registry(GOOD_ROW + GOOD_ROW), "demo:one: duplicate id")

    def test_title_must_be_non_empty(self):
        self.assertFires(_registry(GOOD_ROW.replace('"Demo one"', '"   "')), "title must be a non-empty string")

    def test_seam_must_be_a_string(self):
        self.assertFires(_registry(GOOD_ROW + "seam = 3\n"), "seam must be a string")

    def test_pinned_must_be_a_boolean(self):
        self.assertFires(_registry(GOOD_ROW + 'pinned = "no"\n'), "pinned must be a boolean")

    def test_pinned_false_is_valid(self):
        self.assertClean(_registry(GOOD_ROW + "pinned = false\n"))

    def test_discriminants_must_be_a_table(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('{ marker = "0x01" }', '"marker"')),
            "discriminants must be a table",
        )

    def test_discriminants_must_not_be_empty(self):
        self.assertFires(_registry(GOOD_ROW.replace('{ marker = "0x01" }', "{ }")), "must not be empty")

    def test_discriminant_values_must_be_strings(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('{ marker = "0x01" }', "{ marker = 1 }")),
            "discriminant marker must be a string",
        )


class TestWitness(RegistryCase):
    def test_witness_must_be_a_string(self):
        self.assertFires(_registry(GOOD_ROW.replace('"spec:Demo specification section 1"', "42")), "witness must be a string")

    def test_witness_prefix(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', '"blog:somewhere"')),
            "must start with spec:, corpus:, or code:",
        )

    def test_corpus_witness_needs_a_path(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', '"corpus:  "')),
            "corpus witness names no path",
        )

    def test_corpus_witness_must_be_repo_relative(self):
        for bad in ("/etc/passwd", "../outside.igs"):
            with self.subTest(path=bad):
                self.assertFires(
                    _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', f'"corpus:{bad}"')),
                    "must be a repo-relative path",
                )

    def test_corpus_witness_file_must_exist(self):
        self.assertFires(
            _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', '"corpus:fixtures/demo.bin"')),
            "corpus witness file not found",
        )

    def test_corpus_witness_present_is_clean(self):
        self.assertClean(
            _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', '"corpus:fixtures/demo.bin"')),
            files=["fixtures/demo.bin"],
        )

    def _code(self, witness: str) -> str:
        return _registry(GOOD_ROW.replace('"spec:Demo specification section 1"', f'"{witness}"'))

    def test_code_witness_is_debt_not_an_error(self):
        summary = self.assertClean(self._code("code:src/demo.rs:12"), files=["src/demo.rs"])
        self.assertIn("1 rows on code: witnesses", summary)

    def test_code_witness_file_must_exist(self):
        self.assertFires(self._code("code:src/gone.rs:12"), "code witness file not found: src/gone.rs")

    def test_code_witness_without_a_line_must_exist(self):
        self.assertFires(self._code("code:src/gone.rs"), "code witness file not found: src/gone.rs")

    def test_code_witness_line_is_not_validated(self):
        # Line drift as the codec changes is expected; only the file is checked.
        for witness in ("code:src/demo.rs:1", "code:src/demo.rs:999999", "code:src/demo.rs"):
            with self.subTest(witness=witness):
                self.assertClean(self._code(witness), files=["src/demo.rs"])

    def test_code_witness_needs_a_path(self):
        self.assertFires(self._code("code:  "), "code witness names no path")

    def test_code_witness_must_be_repo_relative(self):
        for bad in ("/etc/passwd:1", "../outside.rs:1"):
            with self.subTest(path=bad):
                self.assertFires(self._code(f"code:{bad}"), "code witness must be a repo-relative path")

    def test_code_witness_directory_is_not_a_file(self):
        self.assertFires(
            self._code("code:src:12"),
            "code witness file not found: src",
            files=["src/demo.rs"],
        )


class TestLattice(RegistryCase):
    IGES_FORMATS = "[format.iges]\ncomplete = true\n[format.demo]\ncomplete = true\n"

    def iges(self, extra: str) -> str:
        base = (
            '[[dialect]]\nid = "iges:4.0-fixed-ascii"\ntitle = "IGES 4.0"\n'
            'discriminants = { effective_version = "4.0" }\nwitness = "spec:IGES 4.0"\n'
            '[[dialect]]\nid = "iges:5.0-fixed-ascii"\ntitle = "IGES 5.0"\n'
            'discriminants = { effective_version = "5.0" }\nwitness = "spec:IGES 5.3"\n'
        )
        return _registry(base + extra, formats=self.IGES_FORMATS)

    def test_lattice_keys_rejected_off_iges(self):
        for key in ("supersedes", "adds", "subtracts"):
            with self.subTest(key=key):
                self.assertFires(
                    _registry(GOOD_ROW + f'{key} = ["402:6"]\n', formats=self.IGES_FORMATS),
                    "admitted only on iges rows",
                )

    def test_lattice_keys_must_be_lists_of_strings(self):
        for key in ("supersedes", "adds", "subtracts"):
            with self.subTest(key=key):
                self.assertFires(self.iges(f"{key} = [7]\n"), f"{key} must be a list of strings")

    def test_subtracts_shape(self):
        self.assertFires(self.iges('subtracts = ["402"]\n'), "is not type:form")

    def test_subtracts_rejects_a_range(self):
        self.assertFires(self.iges('subtracts = ["406:19-26"]\n'), "is not type:form")

    def test_adds_shape(self):
        self.assertFires(self.iges('adds = ["141:x"]\n'), "is not type:form or type:low-high")

    def test_adds_inverted_range(self):
        self.assertFires(self.iges('adds = ["406:26-19"]\n'), "inverted form range")

    def test_adds_accepts_point_and_range(self):
        self.assertClean(self.iges('adds = ["141:0", "406:19-26", "228:5001-9999"]\n'))

    def test_supersedes_unknown_id(self):
        self.assertFires(
            self.iges('supersedes = ["iges:3.0-fixed-ascii"]\n'),
            "supersedes unknown id iges:3.0-fixed-ascii",
        )

    def test_supersedes_resolves(self):
        self.assertClean(self.iges('supersedes = ["iges:4.0-fixed-ascii"]\nsubtracts = ["402:6"]\n'))

    def test_supersedes_self_cycle(self):
        self.assertFires(self.iges('supersedes = ["iges:5.0-fixed-ascii"]\n'), "supersedes cycle")

    def test_supersedes_two_node_cycle(self):
        text = _registry(
            '[[dialect]]\nid = "iges:a"\ntitle = "A"\ndiscriminants = { v = "a" }\n'
            'witness = "spec:s"\nsupersedes = ["iges:b"]\n'
            '[[dialect]]\nid = "iges:b"\ntitle = "B"\ndiscriminants = { v = "b" }\n'
            'witness = "spec:s"\nsupersedes = ["iges:a"]\n',
            formats=self.IGES_FORMATS,
        )
        self.assertFires(text, "supersedes cycle")

    def test_supersedes_chain_terminates(self):
        text = _registry(
            '[[dialect]]\nid = "iges:a"\ntitle = "A"\ndiscriminants = { v = "a" }\nwitness = "spec:s"\n'
            '[[dialect]]\nid = "iges:b"\ntitle = "B"\ndiscriminants = { v = "b" }\n'
            'witness = "spec:s"\nsupersedes = ["iges:a"]\n'
            '[[dialect]]\nid = "iges:c"\ntitle = "C"\ndiscriminants = { v = "c" }\n'
            'witness = "spec:s"\nsupersedes = ["iges:b"]\n',
            formats=self.IGES_FORMATS,
        )
        self.assertClean(text)


class TestExtensionPoints(unittest.TestCase):
    def emitted_id_failures(self, row: str, source: str = ""):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "dialects.toml").write_text(
                _registry(row), encoding="utf-8"
            )
            source_path = root / "crates" / "demo" / "src" / "dialect.rs"
            source_path.parent.mkdir(parents=True)
            source_path.write_text(source, encoding="utf-8")
            return checker.check_codec_emitted_ids(root)

    def test_present_codec_id_passes(self):
        self.assertEqual(self.emitted_id_failures(GOOD_ROW, 'const ID: &str = "demo:one";'), [])

    def test_dead_codec_id_fails(self):
        self.assertEqual(
            self.emitted_id_failures(GOOD_ROW.replace("demo:one", "demo:fabricated")),
            ["demo:fabricated: no string literal under crates/*/src"],
        )

    def test_explicitly_unpinned_row_is_exempt(self):
        self.assertEqual(self.emitted_id_failures(GOOD_ROW + "pinned = false\n"), [])

    def test_support_tables_moved_to_the_renderer(self):
        """The stub is gone because the rule is enforced, not deferred."""
        self.assertFalse(hasattr(checker, "check_support_tables"))
        self.assertTrue((REPO / "scripts" / "render-format-support.py").is_file())

    def test_fixture_gating_moved_to_the_capability_checker(self):
        """The stub is gone because the rule is enforced, not deferred."""
        self.assertFalse(hasattr(checker, "check_fixture_gating"))
        self.assertTrue((REPO / "scripts" / "check-dialect-support.py").is_file())


class TestCommittedRegistry(unittest.TestCase):
    def test_real_registry_is_clean(self):
        failures, summary = checker.check(REPO)
        self.assertEqual(failures, [], "\n".join(failures))
        self.assertTrue(summary.startswith("dialects: ok"))

    def test_main_exits_zero_on_the_real_registry(self):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            code = checker.main([str(REPO)])
        self.assertEqual(code, 0)
        self.assertIn("dialects: ok", out.getvalue())

    def test_main_exits_one_on_a_broken_registry(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "dialects.toml").write_text("nonsense = =\n", encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()) as err:
                code = checker.main([str(root)])
        self.assertEqual(code, 1)
        self.assertIn("error: ", err.getvalue())


if __name__ == "__main__":
    unittest.main()
