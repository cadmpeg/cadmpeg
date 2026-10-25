#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_layout_tables.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_layout_tables")

GENERATOR = ('pub(crate) const GENERATED_LAYOUT_RS: &[(&str, &str)] = &[\n'
             '    ("f3d", "crates/cadmpeg-codec-f3d/src/layout.rs"),\n];\n')
TOML = "docs/layouts/f3d.toml"
PAGE = "docs/layouts/f3d.md"
CODE = "crates/cadmpeg-codec-f3d/src/layout.rs"


class LayoutCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.GENERATOR: GENERATOR, TOML: "a = 1\n", PAGE: "| a |\n",
                          CODE: "pub const A: usize = 1;\n"})
        self.repo.branch()

    def results(self, **options):
        return harness.run_check(check, self.repo.context(**options))

    def test_untouched_passes(self) -> None:
        self.repo.commit({"README.md": "x\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_table_with_derived_files_asks_for_verification(self) -> None:
        self.repo.commit({TOML: "a = 2\n", PAGE: "| b |\n", CODE: "pub const A: usize = 2;\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("UPDATE_LAYOUT_CODE=1", results[0].details[0])

    def test_table_without_page_fails(self) -> None:
        self.repo.commit({TOML: "a = 2\n"})
        self.assertEqual(harness.statuses(self.results()), ["FAIL"])

    def test_hand_edited_derived_file_fails(self) -> None:
        self.repo.commit({CODE: "pub const A: usize = 3;\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn("never edited by hand", results[0].details[0])

    def test_hand_edited_derived_file_points_to_run_layout(self) -> None:
        self.repo.commit({CODE: "pub const A: usize = 3;\n"})
        self.assertIn("--run-layout", " ".join(self.results()[0].details))

    def test_run_layout_passes_an_identical_regeneration(self) -> None:
        self.repo.commit({CODE: "pub const A: usize = 3;\n"})
        context = self.repo.context(run_layout=True)
        context.run = lambda command, env=None: (0, "")
        self.assertEqual(harness.statuses(harness.run_check(check, context)), ["PASS"])

    def test_run_layout_fails_a_hand_edit_regeneration_undoes(self) -> None:
        self.repo.commit({CODE: "pub const A: usize = 3;\n"})
        context = self.repo.context(run_layout=True)

        def regenerate(command, env=None):
            if env:
                (self.repo.root / CODE).write_text("pub const A: usize = 1;\n")
            return 0, ""

        context.run = regenerate
        results = harness.run_check(check, context)
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(CODE, results[0].details)

    def test_run_layout_with_rev_keeps_the_failure(self) -> None:
        self.repo.commit({CODE: "pub const A: usize = 3;\n"})
        results = self.results(rev="HEAD", run_layout=True)
        self.assertEqual(harness.statuses(results), ["FAIL", "SKIP"])
        self.assertIn("never edited by hand", results[0].details[0])

    def test_conflict_markers_fail(self) -> None:
        self.repo.commit({TOML: "<<<<<<< HEAD\na = 2\n=======\na = 3\n>>>>>>> main\n",
                          PAGE: "| b |\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(f"{TOML}: conflict markers", results[0].details)

    def test_run_layout_reports_a_regeneration_diff(self) -> None:
        self.repo.commit({TOML: "a = 2\n", PAGE: "| b |\n"})
        context = self.repo.context(run_layout=True)
        calls = []

        def fake_run(command, env=None):
            calls.append(env)
            if env:
                (self.repo.root / CODE).write_text("pub const A: usize = 2;\n")
            return 0, ""

        context.run = fake_run
        results = harness.run_check(check, context)
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(CODE, results[0].details)
        self.assertEqual(len(calls), 2)

    def test_run_layout_passes_when_nothing_changes(self) -> None:
        self.repo.commit({TOML: "a = 2\n", PAGE: "| b |\n", CODE: "pub const A: usize = 2;\n"})
        context = self.repo.context(run_layout=True)
        context.run = lambda command, env=None: (0, "")
        self.assertEqual(harness.statuses(harness.run_check(check, context)), ["PASS"])


if __name__ == "__main__":
    unittest.main()
